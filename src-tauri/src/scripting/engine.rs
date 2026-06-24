//! QuickJS scripting engine.
//!
//! Each script execution gets a fresh `Runtime` + `Context` pair so scripts
//! cannot share state. The sandbox has no access to the filesystem, network,
//! or system clock — only the `pm` object (injected as a global) and the
//! standard JS built-ins that QuickJS ships.
//!
//! # pm.* API surface
//!
//! ```js
//! // Environment
//! pm.environment.get(key)        // → string | undefined
//! pm.environment.set(key, value) // recorded in ScriptResult.env_mutations
//!
//! // Request (pre-script only)
//! pm.request.method              // string
//! pm.request.url                 // string
//! pm.request.headers.get(name)   // string | undefined
//!
//! // Response (post-script only)
//! pm.response.status             // number
//! pm.response.statusText         // string
//! pm.response.headers.get(name)  // string | undefined
//! pm.response.json()             // parsed JS object / array
//! pm.response.text()             // string
//! pm.response.responseTime       // number (ms)
//!
//! // Testing
//! pm.test(name, fn)              // fn() may throw; caught → failed test
//! pm.expect(value)               // chainable assertion builder
//!
//! // Control
//! pm.abort()                     // cancel the send
//!
//! // Template tags (globals)
//! $guid                          // random UUID v4
//! $timestamp                     // Unix epoch seconds (integer)
//! $randomInt                     // 0–1000 integer
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rquickjs::{
    CatchResultExt, Context, Function, Object, Runtime, Value,
};

use crate::error::{AppError, AppResult};
use super::types::{PostScriptContext, PreScriptContext, ScriptResult, TestResult};

// ---------------------------------------------------------------------------
// Internal shared state mutated by pm.* calls during a script run.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RunState {
    tests: Vec<TestResult>,
    env_mutations: Vec<(String, String)>,
    aborted: bool,
}

type SharedState = Arc<Mutex<RunState>>;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Run a pre-request script.  Returns `ScriptResult::aborted = true` if the
/// script called `pm.abort()`.
pub fn run_pre_script(
    script: &str,
    ctx: &PreScriptContext,
) -> AppResult<ScriptResult> {
    if script.trim().is_empty() {
        return Ok(ScriptResult::default());
    }

    let state: SharedState = Arc::new(Mutex::new(RunState::default()));
    let rt = Runtime::new().map_err(|e| AppError::Script(e.to_string()))?;
    let js_ctx = Context::full(&rt).map_err(|e| AppError::Script(e.to_string()))?;

    let mut result = ScriptResult::default();

    js_ctx.with(|cx| -> rquickjs::Result<()> {
        let globals = cx.globals();

        inject_template_tags(&cx, &globals)?;
        inject_pm_pre(&cx, &globals, ctx, Arc::clone(&state))?;

        match cx.eval::<Value, _>(script.as_bytes()).catch(&cx) {
            Ok(_) => {}
            Err(e) => {
                result.error = Some(e.to_string());
            }
        }
        cx.run_gc();
        Ok(())
    }).map_err(|e| AppError::Script(e.to_string()))?;

    let locked = state.lock().unwrap();
    result.tests = locked.tests.clone();
    result.env_mutations = locked.env_mutations.clone();
    result.aborted = locked.aborted;
    Ok(result)
}

/// Run a post-response script.
pub fn run_post_script(
    script: &str,
    ctx: &PostScriptContext,
) -> AppResult<ScriptResult> {
    if script.trim().is_empty() {
        return Ok(ScriptResult::default());
    }

    let state: SharedState = Arc::new(Mutex::new(RunState::default()));
    let rt = Runtime::new().map_err(|e| AppError::Script(e.to_string()))?;
    let js_ctx = Context::full(&rt).map_err(|e| AppError::Script(e.to_string()))?;

    let mut result = ScriptResult::default();

    js_ctx.with(|cx| -> rquickjs::Result<()> {
        let globals = cx.globals();

        inject_template_tags(&cx, &globals)?;
        inject_pm_post(&cx, &globals, ctx, Arc::clone(&state))?;

        match cx.eval::<Value, _>(script.as_bytes()).catch(&cx) {
            Ok(_) => {}
            Err(e) => {
                result.error = Some(e.to_string());
            }
        }
        cx.run_gc();
        Ok(())
    }).map_err(|e| AppError::Script(e.to_string()))?;

    let locked = state.lock().unwrap();
    result.tests = locked.tests.clone();
    result.env_mutations = locked.env_mutations.clone();
    result.aborted = locked.aborted;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Template tags
// ---------------------------------------------------------------------------

fn inject_template_tags<'js>(
    _cx: &rquickjs::Ctx<'js>,
    globals: &Object<'js>,
) -> rquickjs::Result<()> {
    use uuid::Uuid;

    let guid = Uuid::new_v4().to_string();
    globals.set("$guid", guid)?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    globals.set("$timestamp", ts as i64)?;

    let rand_int = (ts % 1001) as i64; // deterministic-ish, no rand dep needed
    globals.set("$randomInt", rand_int)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// pm object — pre-request variant
// ---------------------------------------------------------------------------

fn inject_pm_pre<'js>(
    cx: &rquickjs::Ctx<'js>,
    globals: &Object<'js>,
    ctx: &PreScriptContext,
    state: SharedState,
) -> rquickjs::Result<()> {
    let pm = Object::new(cx.clone())?;

    // pm.environment
    let env_obj = build_env_object(cx, &ctx.env, Arc::clone(&state))?;
    pm.set("environment", env_obj)?;

    // pm.request (read-only)
    let req_obj = Object::new(cx.clone())?;
    req_obj.set("method", ctx.method.clone())?;
    req_obj.set("url", ctx.url.clone())?;

    let headers_map: HashMap<String, String> = ctx.headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect();
    let hdr_obj = build_headers_object(cx, headers_map)?;
    req_obj.set("headers", hdr_obj)?;
    pm.set("request", req_obj)?;

    // pm.test
    let test_fn = build_test_fn(cx, Arc::clone(&state))?;
    pm.set("test", test_fn)?;

    // pm.expect
    let expect_fn = build_expect_fn(cx)?;
    pm.set("expect", expect_fn)?;

    // pm.abort
    let state_abort = Arc::clone(&state);
    let abort_fn = Function::new(cx.clone(), move || {
        state_abort.lock().unwrap().aborted = true;
    })?;
    pm.set("abort", abort_fn)?;

    globals.set("pm", pm)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// pm object — post-response variant
// ---------------------------------------------------------------------------

fn inject_pm_post<'js>(
    cx: &rquickjs::Ctx<'js>,
    globals: &Object<'js>,
    ctx: &PostScriptContext,
    state: SharedState,
) -> rquickjs::Result<()> {
    let pm = Object::new(cx.clone())?;

    // pm.environment
    let env_obj = build_env_object(cx, &ctx.env, Arc::clone(&state))?;
    pm.set("environment", env_obj)?;

    // pm.request (read-only, just method + url)
    let req_obj = Object::new(cx.clone())?;
    req_obj.set("method", ctx.method.clone())?;
    req_obj.set("url", ctx.url.clone())?;
    let req_headers: HashMap<String, String> = ctx.request_headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect();
    req_obj.set("headers", build_headers_object(cx, req_headers)?)?;
    pm.set("request", req_obj)?;

    // pm.response
    let resp_obj = Object::new(cx.clone())?;
    resp_obj.set("status", ctx.status as i32)?;
    resp_obj.set("statusText", ctx.status_text.clone())?;
    resp_obj.set("responseTime", ctx.duration_ms)?;

    let resp_headers: HashMap<String, String> = ctx.response_headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect();
    resp_obj.set("headers", build_headers_object(cx, resp_headers)?)?;

    // pm.response.text() → string
    let body_clone = ctx.body.clone();
    let text_fn = Function::new(cx.clone(), move || body_clone.clone())?;
    resp_obj.set("text", text_fn)?;

    // pm.response.json() → parsed value (throws if not valid JSON).
    // Takes a fresh per-call `Ctx` parameter (zero JS-visible args, per
    // rquickjs's `FromParam<Ctx>` impl) rather than capturing a `Ctx` clone —
    // a captured clone lives in the closure for the JS heap's lifetime and
    // forms a context-refcount cycle the JS GC can't see, aborting on teardown.
    let body_for_json = ctx.body.clone();
    let json_fn = Function::new(
        cx.clone(),
        correlate_ctx(move |cx: rquickjs::Ctx<'_>| cx.json_parse(body_for_json.as_bytes())),
    )?;
    resp_obj.set("json", json_fn)?;

    pm.set("response", resp_obj)?;

    // pm.test
    pm.set("test", build_test_fn(cx, Arc::clone(&state))?)?;

    // pm.expect
    pm.set("expect", build_expect_fn(cx)?)?;

    // pm.abort
    let state_abort = Arc::clone(&state);
    pm.set("abort", Function::new(cx.clone(), move || {
        state_abort.lock().unwrap().aborted = true;
    })?)?;

    globals.set("pm", pm)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_env_object<'js>(
    cx: &rquickjs::Ctx<'js>,
    env: &HashMap<String, String>,
    state: SharedState,
) -> rquickjs::Result<Object<'js>> {
    let obj = Object::new(cx.clone())?;

    // get(key) → string | undefined
    let snapshot: HashMap<String, String> = env.clone();
    let state_get = Arc::clone(&state);
    let get_fn = Function::new(cx.clone(), move |key: String| -> Option<String> {
        // Check mutations first (script may have set it earlier)
        let locked = state_get.lock().unwrap();
        for (k, v) in locked.env_mutations.iter().rev() {
            if *k == key {
                return Some(v.clone());
            }
        }
        snapshot.get(&key).cloned()
    })?;
    obj.set("get", get_fn)?;

    // set(key, value)
    let state_set = Arc::clone(&state);
    let set_fn = Function::new(cx.clone(), move |key: String, value: String| {
        state_set.lock().unwrap().env_mutations.push((key, value));
    })?;
    obj.set("set", set_fn)?;

    Ok(obj)
}

fn build_headers_object<'js>(
    cx: &rquickjs::Ctx<'js>,
    map: HashMap<String, String>,
) -> rquickjs::Result<Object<'js>> {
    let obj = Object::new(cx.clone())?;
    let get_fn = Function::new(cx.clone(), move |name: String| -> Option<String> {
        map.get(&name.to_lowercase()).cloned()
    })?;
    obj.set("get", get_fn)?;
    Ok(obj)
}

/// pm.test(name, fn) — fn() is called; any thrown error is caught and
/// recorded as a failure.
fn build_test_fn<'js>(
    cx: &rquickjs::Ctx<'js>,
    state: SharedState,
) -> rquickjs::Result<Function<'js>> {
    let f = Function::new(
        cx.clone(),
        move |cx: rquickjs::Ctx<'_>, name: String, cb: Function<'_>| {
            let res: rquickjs::Result<Value<'_>> = cb.call(());
            let error = match res.catch(&cx) {
                Ok(_) => None,
                // Real `Error` instances (the common case — every assertion
                // throws via `Exception::throw_message`) get just the bare
                // message, not `Display`'s `"Error: {message}\n{stack}"`.
                Err(rquickjs::CaughtError::Exception(ex)) => Some(ex.message().unwrap_or_else(|| ex.to_string())),
                Err(other) => Some(other.to_string()),
            };
            let passed = error.is_none();
            state.lock().unwrap().tests.push(TestResult { name, passed, error });
        },
    )?;
    Ok(f)
}

/// pm.expect(value) — returns a chainable assertion object.
/// Supports: .to.equal(v), .to.be.true, .to.be.false, .to.include(s),
///           .to.have.length(n), .to.be.null, .to.be.undefined,
///           .to.be.a(type_str).
fn build_expect_fn<'js>(cx: &rquickjs::Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    let f = Function::new(
        cx.clone(),
        correlate(move |val: Value<'_>| {
            let cx = val.ctx().clone();
            build_assertion_chain(&cx, val)
        }),
    )?;
    Ok(f)
}

/// Helper that forces rustc to type a closure as `for<'a> Fn(Value<'a>) ->
/// Result<Object<'a>>` — i.e. input and output share one lifetime. Plain closure
/// inference treats each elided `'_` as an independent region, which `Object`'s
/// invariance then rejects; this identity function supplies the missing HRTB.
fn correlate<F>(f: F) -> F
where
    F: for<'a> Fn(Value<'a>) -> rquickjs::Result<Object<'a>>,
{
    f
}

/// Same as `correlate`, but for closures shaped `Ctx -> Result<Value>`
/// (e.g. `pm.response.json()`).
fn correlate_ctx<F>(f: F) -> F
where
    F: for<'a> Fn(rquickjs::Ctx<'a>) -> rquickjs::Result<Value<'a>>,
{
    f
}

/// Builds the chainable object returned by `pm.expect()`. Each leaf assertion
/// throws on failure rather than recording its own `TestResult` — `pm.test`'s
/// wrapper (`build_test_fn`) is the single place a result gets recorded, by
/// catching that throw. Recording in both places double-counted every test.
fn build_assertion_chain<'js>(
    cx: &rquickjs::Ctx<'js>,
    subject: Value<'js>,
) -> rquickjs::Result<Object<'js>> {
    let chain = Object::new(cx.clone())?;

    // .equal(expected)
    {
        let subj = subject.clone();
        let eq_fn = Function::new(
            cx.clone(),
            move |cx: rquickjs::Ctx<'_>, expected: Value<'_>| -> rquickjs::Result<()> {
                if js_values_equal(&subj, &expected) {
                    return Ok(());
                }
                let msg = format!(
                    "Expected {:?} to equal {:?}",
                    js_to_debug(&subj),
                    js_to_debug(&expected)
                );
                Err(rquickjs::Exception::throw_message(&cx, &msg))
            },
        )?;
        chain.set("equal", eq_fn)?;
    }

    // .include(substring / element)
    {
        let subj = subject.clone();
        let inc_fn = Function::new(
            cx.clone(),
            move |cx: rquickjs::Ctx<'_>, needle: Value<'_>| -> rquickjs::Result<()> {
                if js_includes(&subj, &needle) {
                    return Ok(());
                }
                let msg = format!(
                    "Expected {:?} to include {:?}",
                    js_to_debug(&subj),
                    js_to_debug(&needle)
                );
                Err(rquickjs::Exception::throw_message(&cx, &msg))
            },
        )?;
        chain.set("include", inc_fn)?;
    }

    // .to — returns same chain (fluent)
    chain.set("to", chain.clone())?;

    // .be — returns same chain
    chain.set("be", chain.clone())?;

    // .have — returns same chain
    chain.set("have", chain.clone())?;

    // .not — TODO: negation chain (future)
    chain.set("not", chain.clone())?;

    // .true / .false / .null / .undefined (getter-like properties, JS boolean)
    {
        let subj = subject.clone();
        // We expose these as functions so they can be invoked without property getters
        let true_fn = Function::new(cx.clone(), move |cx: rquickjs::Ctx<'_>| -> rquickjs::Result<()> {
            if subj.as_bool() == Some(true) {
                return Ok(());
            }
            Err(rquickjs::Exception::throw_message(
                &cx,
                &format!("Expected {:?} to be true", js_to_debug(&subj)),
            ))
        })?;
        chain.set("true", true_fn)?;
    }
    {
        let subj = subject.clone();
        let false_fn = Function::new(cx.clone(), move |cx: rquickjs::Ctx<'_>| -> rquickjs::Result<()> {
            if subj.as_bool() == Some(false) {
                return Ok(());
            }
            Err(rquickjs::Exception::throw_message(
                &cx,
                &format!("Expected {:?} to be false", js_to_debug(&subj)),
            ))
        })?;
        chain.set("false", false_fn)?;
    }
    {
        let subj = subject.clone();
        let null_fn = Function::new(cx.clone(), move |cx: rquickjs::Ctx<'_>| -> rquickjs::Result<()> {
            if subj.is_null() {
                return Ok(());
            }
            Err(rquickjs::Exception::throw_message(
                &cx,
                &format!("Expected {:?} to be null", js_to_debug(&subj)),
            ))
        })?;
        chain.set("null", null_fn)?;
    }
    {
        let subj = subject.clone();
        let undef_fn = Function::new(cx.clone(), move |cx: rquickjs::Ctx<'_>| -> rquickjs::Result<()> {
            if subj.is_undefined() {
                return Ok(());
            }
            Err(rquickjs::Exception::throw_message(
                &cx,
                &format!("Expected {:?} to be undefined", js_to_debug(&subj)),
            ))
        })?;
        chain.set("undefined", undef_fn)?;
    }

    // .a(type_str) / .an(type_str)
    {
        let subj = subject.clone();
        let a_fn = Function::new(
            cx.clone(),
            move |cx: rquickjs::Ctx<'_>, type_str: String| -> rquickjs::Result<()> {
                let actual = js_typeof(&subj);
                if actual == type_str {
                    return Ok(());
                }
                let msg = format!("Expected type {:?}, got {:?}", type_str, actual);
                Err(rquickjs::Exception::throw_message(&cx, &msg))
            },
        )?;
        chain.set("a", a_fn.clone())?;
        chain.set("an", a_fn)?;
    }

    // .length(n)
    {
        let subj = subject.clone();
        let len_fn = Function::new(
            cx.clone(),
            move |cx: rquickjs::Ctx<'_>, n: i32| -> rquickjs::Result<()> {
                let actual = js_length(&subj);
                if actual == Some(n as usize) {
                    return Ok(());
                }
                let msg = format!("Expected length {n}, got {:?}", actual);
                Err(rquickjs::Exception::throw_message(&cx, &msg))
            },
        )?;
        chain.set("length", len_fn)?;
    }

    Ok(chain)
}

// ---------------------------------------------------------------------------
// JS value utilities
// ---------------------------------------------------------------------------

fn js_to_debug(v: &Value<'_>) -> String {
    if v.is_string() {
        v.as_string().and_then(|s| s.to_string().ok())
            .map(|s| format!("\"{s}\""))
            .unwrap_or_else(|| "?".into())
    } else if v.is_int() {
        v.as_int().map(|n| n.to_string()).unwrap_or_else(|| "?".into())
    } else if v.is_float() {
        v.as_float().map(|f| f.to_string()).unwrap_or_else(|| "?".into())
    } else if v.as_bool().is_some() {
        v.as_bool().unwrap().to_string()
    } else if v.is_null() {
        "null".into()
    } else if v.is_undefined() {
        "undefined".into()
    } else {
        "object".into()
    }
}

fn js_values_equal(a: &Value<'_>, b: &Value<'_>) -> bool {
    if a.is_string() && b.is_string() {
        let sa = a.as_string().and_then(|s| s.to_string().ok());
        let sb = b.as_string().and_then(|s| s.to_string().ok());
        return sa == sb;
    }
    if a.is_int() && b.is_int() {
        return a.as_int() == b.as_int();
    }
    if (a.is_int() || a.is_float()) && (b.is_int() || b.is_float()) {
        let fa = a.as_float()
            .or_else(|| a.as_int().map(|n| n as f64));
        let fb = b.as_float()
            .or_else(|| b.as_int().map(|n| n as f64));
        return fa == fb;
    }
    if a.as_bool().is_some() && b.as_bool().is_some() {
        return a.as_bool() == b.as_bool();
    }
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_undefined() && b.is_undefined() {
        return true;
    }
    false
}

fn js_includes(haystack: &Value<'_>, needle: &Value<'_>) -> bool {
    if let (Some(hs), Some(n)) = (
        haystack.as_string().and_then(|s| s.to_string().ok()),
        needle.as_string().and_then(|s| s.to_string().ok()),
    ) {
        return hs.contains(&n);
    }
    false
}

fn js_typeof(v: &Value<'_>) -> String {
    if v.is_string() { "string".into() }
    else if v.is_int() || v.is_float() { "number".into() }
    else if v.as_bool().is_some() { "boolean".into() }
    else if v.is_null() { "object".into() } // JS quirk
    else if v.is_undefined() { "undefined".into() }
    else if v.is_function() { "function".into() }
    else { "object".into() }
}

fn js_length(v: &Value<'_>) -> Option<usize> {
    if let Some(s) = v.as_string().and_then(|s| s.to_string().ok()) {
        return Some(s.len());
    }
    // For arrays/objects we'd need to access .length property — not yet.
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_pre_ctx(env: HashMap<&str, &str>) -> PreScriptContext {
        PreScriptContext {
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: vec![],
            query: vec![],
            env: env.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    fn make_post_ctx(status: u16, body: &str) -> PostScriptContext {
        PostScriptContext {
            method: "GET".into(),
            url: "https://example.com".into(),
            request_headers: vec![],
            status,
            status_text: "OK".into(),
            response_headers: vec![
                ("content-type".into(), "application/json".into()),
            ],
            body: body.to_string(),
            duration_ms: 42.0,
            env: HashMap::new(),
        }
    }

    #[test]
    fn empty_script_returns_default() {
        let r = run_pre_script("", &make_pre_ctx(HashMap::new())).unwrap();
        assert!(r.tests.is_empty());
        assert!(!r.aborted);
    }

    #[test]
    fn pm_test_pass() {
        let r = run_post_script(
            r#"pm.test("status is 200", function() { pm.expect(pm.response.status).to.equal(200); });"#,
            &make_post_ctx(200, "{}"),
        ).unwrap();
        assert_eq!(r.tests.len(), 1);
        assert!(r.tests[0].passed, "{:?}", r.tests[0].error);
    }

    #[test]
    fn pm_test_fail() {
        let r = run_post_script(
            r#"pm.test("status is 404", function() { pm.expect(pm.response.status).to.equal(404); });"#,
            &make_post_ctx(200, "{}"),
        ).unwrap();
        assert_eq!(r.tests.len(), 1);
        assert!(!r.tests[0].passed);
    }

    #[test]
    fn pm_response_json_and_test() {
        let r = run_post_script(
            r#"
            var data = pm.response.json();
            pm.test("has token", function() {
                pm.expect(data.token).to.equal("abc");
            });
            "#,
            &make_post_ctx(200, r#"{"token":"abc"}"#),
        ).unwrap();
        assert!(r.tests[0].passed, "{:?}", r.tests[0].error);
    }

    #[test]
    fn pm_environment_get_set() {
        let mut env = HashMap::new();
        env.insert("base_url", "https://api.example.com");
        let r = run_pre_script(
            r#"
            var url = pm.environment.get("base_url");
            pm.environment.set("token", "tok-123");
            pm.test("url set", function() { pm.expect(url).to.equal("https://api.example.com"); });
            "#,
            &make_pre_ctx(env),
        ).unwrap();
        assert!(r.tests[0].passed);
        assert_eq!(r.env_mutations, vec![("token".to_string(), "tok-123".to_string())]);
    }

    #[test]
    fn pm_abort_sets_flag() {
        let r = run_pre_script("pm.abort();", &make_pre_ctx(HashMap::new())).unwrap();
        assert!(r.aborted);
    }

    #[test]
    fn sandbox_no_require_access() {
        // QuickJS doesn't expose require or XMLHttpRequest — any attempt to
        // call a non-existent global should be a ReferenceError, not crash.
        let r = run_pre_script(
            r#"try { require("fs"); } catch(e) { pm.environment.set("caught", "yes"); }"#,
            &make_pre_ctx(HashMap::new()),
        ).unwrap();
        assert!(r.env_mutations.iter().any(|(k, v)| k == "caught" && v == "yes"));
    }

    #[test]
    fn template_tags_are_globals() {
        let r = run_pre_script(
            r#"pm.test("guid is string", function() { pm.expect(typeof $guid).to.a("string"); });"#,
            &make_pre_ctx(HashMap::new()),
        ).unwrap();
        assert!(r.tests[0].passed, "{:?}", r.tests[0].error);
    }

}
