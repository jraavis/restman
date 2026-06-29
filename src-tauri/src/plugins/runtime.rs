//! Sandboxed execution of a single named function from plugin JS source.
//!
//! Each call gets a fresh `Runtime` + `Context` pair (no state leaks between
//! calls, matching `scripting::engine`'s per-script isolation) and reuses
//! that engine's `apply_runtime_limits` for the 8s wall-clock timeout and
//! 512KB stack ceiling. The sandbox has no filesystem/network access by
//! construction — QuickJS simply doesn't expose those globals.
//!
//! The two public entry points differ only in how they marshal the JS
//! return value back into Rust:
//! - `call_returning_string` expects (and requires) a JS string.
//! - `call_returning_json` expects a JSON-serializable value and
//!   round-trips it through `JSON.stringify` + `serde_json::from_str`.

use rquickjs::{CatchResultExt, Context, Function, Object, Runtime, Value};
use serde::de::DeserializeOwned;

use crate::error::{AppError, AppResult};
use crate::scripting::engine::apply_runtime_limits;

/// Rewrite the rquickjs "interrupted" error into the friendlier message used
/// across the sandbox — same rewrite `scripting::engine` applies for
/// pre/post scripts, duplicated here rather than threading a shared helper
/// through a `pub(crate)` boundary for two call sites.
fn friendly_error(e: rquickjs::CaughtError<'_>) -> AppError {
    let msg = e.to_string();
    if msg.contains("interrupted") {
        AppError::Script("script timed out after 8s".to_string())
    } else {
        AppError::Script(msg)
    }
}

/// Same rewrite as `friendly_error`, for plain (uncaught-exception)
/// `rquickjs::Error`s raised by API calls like `json_parse` or `Object::get`
/// that don't go through `.catch(&cx)`.
fn script_error(e: rquickjs::Error) -> AppError {
    let msg = e.to_string();
    if msg.contains("interrupted") {
        AppError::Script("script timed out after 8s".to_string())
    } else {
        AppError::Script(msg)
    }
}

/// Evaluate `source` in a fresh sandboxed context, look up `fn_name` as a
/// top-level function, call it with `args` (each converted from JSON), and
/// return the raw JS return value plus the `Ctx` needed to convert it.
///
/// Internal — callers go through `call_returning_string` /
/// `call_returning_json`, which differ only in how they marshal the result.
fn eval_and_call<R>(
    source: &str,
    fn_name: &str,
    args: &[serde_json::Value],
    marshal: impl for<'js> FnOnce(rquickjs::Ctx<'js>, Value<'js>) -> AppResult<R>,
) -> AppResult<R> {
    let rt = Runtime::new().map_err(|e| AppError::Script(e.to_string()))?;
    apply_runtime_limits(&rt);
    let js_ctx = Context::full(&rt).map_err(|e| AppError::Script(e.to_string()))?;

    js_ctx.with(|cx| -> AppResult<R> {
        let globals = cx.globals();

        // 1. Evaluate the plugin source — defines top-level functions/vars
        //    on the global object.
        cx.eval::<Value, _>(source.as_bytes())
            .catch(&cx)
            .map_err(friendly_error)?;

        // 2. Look up the named function and confirm it's callable.
        let func: Function = globals.get(fn_name).map_err(|_| {
            AppError::Script(format!(
                "plugin must define a top-level `{fn_name}` function"
            ))
        })?;

        // 3. Convert each JSON arg into an rquickjs::Value via JSON.parse —
        //    works uniformly for strings/objects/arrays/numbers.
        let mut call_args = rquickjs::function::Args::new(cx.clone(), args.len());
        for arg in args {
            let json = serde_json::to_string(arg)?;
            let js_val = cx.json_parse(json.as_bytes()).map_err(script_error)?;
            call_args.push_arg(js_val).map_err(script_error)?;
        }

        // 4. Call the function and catch JS exceptions / timeouts the same
        //    way the source eval above does.
        let ret: Value = func.call_arg(call_args).catch(&cx).map_err(friendly_error)?;

        // 5. Marshal the JS return value into the caller's desired shape.
        marshal(cx, ret)
    })
}

/// Runs `fn_name(args...)` defined by `source`, in a fresh sandboxed
/// QuickJS Runtime+Context (no fs/network access, 8s timeout, 512KB stack —
/// same limits as the pre/post-request script sandbox). `args` are
/// JSON-serializable values passed positionally to the JS function.
///
/// The function's return value must be a JS string; anything else is a
/// clear `AppError`, not a silent coercion.
pub fn call_returning_string(
    source: &str,
    fn_name: &str,
    args: &[serde_json::Value],
) -> AppResult<String> {
    eval_and_call(source, fn_name, args, |_cx, ret| {
        if let Some(s) = ret.as_string() {
            s.to_string().map_err(|e| AppError::Script(e.to_string()))
        } else {
            Err(AppError::Script(format!(
                "plugin's `{fn_name}` function must return a string, got {}",
                js_typeof(&ret)
            )))
        }
    })
}

/// Same as `call_returning_string`, but expects (and JSON-round-trips) a
/// structured return value. The JS return value is serialized via
/// `JSON.stringify`, then deserialized into `T` with `serde_json::from_str`.
/// A shape mismatch (missing/extra/wrong-typed fields) surfaces serde_json's
/// own message, naming `fn_name`, so a plugin author gets an actionable
/// error rather than a panic.
pub fn call_returning_json<T: DeserializeOwned>(
    source: &str,
    fn_name: &str,
    args: &[serde_json::Value],
) -> AppResult<T> {
    eval_and_call(source, fn_name, args, |cx, ret| {
        let json_obj: Object = cx.globals().get("JSON").map_err(script_error)?;
        let stringify: Function = json_obj.get("stringify").map_err(script_error)?;
        let json_str: Option<String> = stringify.call((ret,)).catch(&cx).map_err(friendly_error)?;

        let json_str = json_str.ok_or_else(|| {
            AppError::Script(format!(
                "plugin's `{fn_name}` function must return a JSON-serializable value, got a value that JSON.stringify cannot represent (e.g. undefined)"
            ))
        })?;

        serde_json::from_str::<T>(&json_str).map_err(|e| {
            AppError::Script(format!(
                "plugin's `{fn_name}` function returned a value that doesn't match the \
                 expected shape: {e} (raw JSON: {json_str})"
            ))
        })
    })
}

fn js_typeof(v: &Value<'_>) -> &'static str {
    if v.is_string() {
        "string"
    } else if v.is_int() || v.is_float() {
        "number"
    } else if v.as_bool().is_some() {
        "boolean"
    } else if v.is_null() {
        "null"
    } else if v.is_undefined() {
        "undefined"
    } else if v.is_function() {
        "function"
    } else if v.is_array() {
        "array"
    } else {
        "object"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::time::Instant;

    #[test]
    fn trivial_plugin_returns_string() {
        let r = call_returning_string(
            r#"function generate() { return "hello plugin"; }"#,
            "generate",
            &[],
        )
        .unwrap();
        assert_eq!(r, "hello plugin");
    }

    #[test]
    fn plugin_uses_its_argument() {
        let arg = serde_json::json!({ "method": "GET", "url": "https://example.com" });
        let r = call_returning_string(
            r#"function generate(req) { return req.method + " " + req.url; }"#,
            "generate",
            &[arg],
        )
        .unwrap();
        assert_eq!(r, "GET https://example.com");
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestPlugin {
        name: String,
        tags: Vec<String>,
        meta: TestMeta,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestMeta {
        count: i64,
    }

    #[test]
    fn plugin_returns_structured_object() {
        let r: TestPlugin = call_returning_json(
            r#"
            function generate() {
                return { name: "demo", tags: ["a", "b"], meta: { count: 2 } };
            }
            "#,
            "generate",
            &[],
        )
        .unwrap();
        assert_eq!(
            r,
            TestPlugin {
                name: "demo".into(),
                tags: vec!["a".into(), "b".into()],
                meta: TestMeta { count: 2 },
            }
        );
    }

    #[test]
    fn missing_function_name_is_clear_error() {
        let err = call_returning_string(r#"function notGenerate() { return "x"; }"#, "generate", &[])
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("generate"),
            "expected error to name the missing function, got: {msg}"
        );
    }

    #[test]
    fn js_exception_surfaces_clear_error() {
        let err = call_returning_string(
            r#"function generate() { throw new Error("boom"); }"#,
            "generate",
            &[],
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("boom"),
            "expected error to surface the JS exception message, got: {msg}"
        );
    }

    #[test]
    fn wrong_return_type_is_clear_error() {
        let err = call_returning_string(r#"function generate() { return 42; }"#, "generate", &[])
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("string") && msg.contains("number"),
            "expected error to mention expected vs actual type, got: {msg}"
        );
    }

    #[test]
    fn malformed_json_shape_is_clear_error() {
        let err: AppResult<TestPlugin> = call_returning_json(
            r#"function generate() { return { wrongField: 1 }; }"#,
            "generate",
            &[],
        );
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("generate"),
            "expected error to name the function, got: {msg}"
        );
    }

    #[test]
    fn infinite_loop_in_top_level_source_is_interrupted() {
        // Same trick as scripting::engine::tests::infinite_loop_is_interrupted_after_the_deadline:
        // build the Runtime directly with an immediate deadline rather than
        // waiting out the real 8s SCRIPT_TIMEOUT.
        let rt = rquickjs::Runtime::new().unwrap();
        let deadline = Instant::now() + std::time::Duration::from_millis(1);
        rt.set_max_stack_size(512 * 1024);
        rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let cx = rquickjs::Context::full(&rt).unwrap();
        let result: Result<(), rquickjs::Error> =
            cx.with(|c| c.eval::<(), _>("while(true){}".as_bytes()));
        assert!(
            result.is_err(),
            "infinite loop in plugin source should have been interrupted, not run forever"
        );
    }

    #[test]
    fn infinite_loop_in_called_function_is_interrupted() {
        let rt = rquickjs::Runtime::new().unwrap();
        let deadline = Instant::now() + std::time::Duration::from_millis(1);
        rt.set_max_stack_size(512 * 1024);
        rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let cx = rquickjs::Context::full(&rt).unwrap();
        let result: Result<(), rquickjs::Error> = cx.with(|c| {
            c.eval::<Value, _>(r#"function generate() { while(true){} }"#.as_bytes())?;
            let func: Function = c.globals().get("generate")?;
            func.call(())
        });
        assert!(
            result.is_err(),
            "infinite loop inside the called function should have been interrupted, not run forever"
        );
    }

    #[test]
    fn sandbox_no_require_access() {
        // QuickJS doesn't expose `require` — calling it should throw a
        // ReferenceError caught by the plugin source itself, not crash.
        let r = call_returning_string(
            r#"
            function generate() {
                try { require("fs"); return "should not reach here"; }
                catch (e) { return "caught"; }
            }
            "#,
            "generate",
            &[],
        )
        .unwrap();
        assert_eq!(r, "caught");
    }
}
