//! IPC commands for the scripting / test-runner subsystem.

use crate::error::{AppError, AppResult};
use crate::model::SavedRequest;
use crate::scripting::TestResult;
use crate::store::{requests, AppState};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

/// Per-request outcome emitted as a Tauri event while the runner is live.
/// Event name: `runner:progress`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerProgress {
    pub run_id: String,
    pub request_id: String,
    pub request_name: String,
    pub index: usize,
    pub total: usize,
    /// None = still in progress, Some = finished (pass/fail detail inside).
    pub result: Option<RequestRunResult>,
}

/// Completed outcome for one request in a collection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRunResult {
    pub status: Option<u16>,
    pub duration_ms: f64,
    pub passed: usize,
    pub failed: usize,
    pub tests: Vec<TestResult>,
    pub error: Option<String>,
}

/// Overall summary returned when the collection run finishes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRunSummary {
    pub run_id: String,
    pub total_requests: usize,
    pub passed_requests: usize,
    pub failed_requests: usize,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub duration_ms: f64,
    pub results: Vec<RequestRunResult>,
    /// JUnit XML export (generated on completion).
    pub junit_xml: String,
}

/// Options for a collection test run.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRunOptions {
    pub workspace_id: String,
    pub collection_id: String,
    /// Optional CSV or JSON string for data-driven iteration.
    /// CSV: first row is headers, each subsequent row is one iteration.
    /// JSON: array of objects, each object provides one iteration's variables.
    pub data: Option<String>,
    /// Number of iterations when no data file is provided (default: 1).
    #[serde(default = "default_iterations")]
    pub iterations: usize,
    /// Delay between requests in milliseconds (default: 0). Ignored when
    /// `parallel` is true — pacing and concurrency don't mix.
    #[serde(default)]
    pub delay_ms: u64,
    /// Run each iteration's requests concurrently (up to `MAX_PARALLEL` at a
    /// time, in waves) instead of one at a time (default: false).
    #[serde(default)]
    pub parallel: bool,
}

fn default_iterations() -> usize {
    1
}

/// Cap on requests in flight at once when `parallel` is true. Requests
/// within an iteration run in waves of this size — wave N+1 doesn't start
/// until every request in wave N has finished — rather than a sliding
/// window, so this needs no semaphore/extra tokio feature.
const MAX_PARALLEL: usize = 5;

/// Run all requests in a collection, emitting `runner:progress` events as
/// each request completes and returning a full `CollectionRunSummary`.
///
/// Each request is sent exactly like a manual send: variable resolution,
/// auth, pre/post scripts.  The shared cookie jar is intentionally NOT used
/// here (each run starts fresh) to keep runs reproducible.
#[tauri::command]
pub async fn run_collection_tests(
    app: AppHandle,
    state: State<'_, AppState>,
    options: CollectionRunOptions,
) -> AppResult<CollectionRunSummary> {
    let run_id = uuid::Uuid::new_v4().to_string();
    let start = std::time::Instant::now();

    // Load all requests in this collection (first level only — nested folders
    // are a follow-up; the runner iterates direct children for now).
    let all_requests = {
        let conn = state.db.lock().unwrap();
        requests::list_by_collection(&conn, &options.collection_id)?
    };
    let total = all_requests.len();

    // Parse data rows for data-driven runs.
    let data_rows: Vec<std::collections::HashMap<String, String>> =
        parse_data(options.data.as_deref())?;
    let iteration_count = if data_rows.is_empty() {
        options.iterations.max(1)
    } else {
        data_rows.len()
    };

    let mut all_results: Vec<RequestRunResult> = Vec::new();

    for iteration in 0..iteration_count {
        let data_vars: std::collections::HashMap<String, String> =
            data_rows.get(iteration).cloned().unwrap_or_default();

        if options.parallel {
            // Waves of up to MAX_PARALLEL: every request in a wave is spawned
            // as its own task and they run concurrently, but the next wave
            // doesn't start until this one's handles are all awaited. They're
            // awaited in submission order (== `all_requests` order), so
            // `all_results` stays index-aligned with `all_requests` for the
            // JUnit export below even though the requests themselves finish
            // out of order.
            let indexed: Vec<(usize, &SavedRequest)> = all_requests.iter().enumerate().collect();
            for chunk in indexed.chunks(MAX_PARALLEL) {
                let mut handles = Vec::with_capacity(chunk.len());
                for &(idx, saved) in chunk {
                    handles.push(tauri::async_runtime::spawn(run_one_request(
                        app.clone(),
                        run_id.clone(),
                        options.workspace_id.clone(),
                        options.collection_id.clone(),
                        saved.clone(),
                        idx,
                        total,
                        0, // delay_ms doesn't apply between concurrent requests
                        data_vars.clone(),
                    )));
                }
                for handle in handles {
                    let result = handle
                        .await
                        .map_err(|e| AppError::Other(format!("collection runner task failed: {e}")))??;
                    all_results.push(result);
                }
            }
        } else {
            for (idx, saved) in all_requests.iter().enumerate() {
                let result = run_one_request(
                    app.clone(),
                    run_id.clone(),
                    options.workspace_id.clone(),
                    options.collection_id.clone(),
                    saved.clone(),
                    idx,
                    total,
                    options.delay_ms,
                    data_vars.clone(),
                )
                .await?;
                all_results.push(result);
            }
        }
    }

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    let total_tests: usize = all_results.iter().map(|r| r.tests.len()).sum();
    let passed_tests: usize = all_results.iter().map(|r| r.passed).sum();
    let failed_tests: usize = all_results.iter().map(|r| r.failed).sum();
    let passed_requests = all_results.iter().filter(|r| r.failed == 0 && r.error.is_none()).count();
    let failed_requests = all_results.len() - passed_requests;

    let junit_xml = build_junit_xml(
        &run_id,
        &all_requests.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        &all_results,
    );

    Ok(CollectionRunSummary {
        run_id,
        total_requests: total,
        passed_requests,
        failed_requests,
        total_tests,
        passed_tests,
        failed_tests,
        duration_ms,
        results: all_results,
        junit_xml,
    })
}

/// Runs one request exactly like a manual send (variable resolution, auth,
/// pre/post scripts) and emits its `runner:progress` start/finish events.
/// Takes owned data and fetches `AppState` from `app` itself (rather than a
/// `State` extractor) so it can also run standalone inside a task spawned by
/// `tauri::async_runtime::spawn` — each `state.db.lock()` below is scoped to
/// end before the next `.await`, since the `MutexGuard` it returns is `!Send`
/// (see `AppState`'s doc comment).
async fn run_one_request(
    app: AppHandle,
    run_id: String,
    workspace_id: String,
    collection_id: String,
    saved: SavedRequest,
    idx: usize,
    total: usize,
    delay_ms: u64,
    data_vars: std::collections::HashMap<String, String>,
) -> AppResult<RequestRunResult> {
    use crate::scripting::{run_post_script, run_pre_script, PostScriptContext, PreScriptContext};
    use crate::vars;
    use base64::Engine as _;

    let state = app.state::<AppState>();

    // Emit in-progress notification.
    let _ = app.emit(
        "runner:progress",
        RunnerProgress {
            run_id: run_id.clone(),
            request_id: saved.id.clone(),
            request_name: saved.name.clone(),
            index: idx,
            total,
            result: None,
        },
    );

    if delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }

    // Resolve vars (workspace + collection scope) + merge data row.
    let mut resolved = {
        let conn = state.db.lock().unwrap();
        vars::resolve(&conn, &workspace_id, Some(&collection_id))?
    };
    for (k, v) in &data_vars {
        resolved.values.insert(k.clone(), v.clone());
    }

    // Build a live HttpRequest from the saved one.
    let mut req = crate::model::http::HttpRequest {
        method: saved.method.clone(),
        url: saved.url.clone(),
        headers: saved.headers.clone(),
        query: saved.query.clone(),
        body: saved.body.clone(),
        options: saved.options.clone(),
        auth: crate::model::auth::AuthConfig::None,
    };
    vars::interpolate_request(&mut req, &resolved.values);

    // Resolve auth (no cookie jar for runner).
    let auth_result = {
        let conn = state.db.lock().unwrap();
        let collection_auth =
            Some((&collection_id as &str, crate::store::collections::get(&conn, &collection_id)?.auth));
        let (owner, masked) = crate::auth::resolve(
            collection_auth,
            saved.auth.clone(),
            &saved.id,
        );
        crate::auth::hydrate(&owner, masked)
    };
    match auth_result {
        Ok(auth) => req.auth = auth,
        Err(e) => {
            let result = RequestRunResult {
                status: None,
                duration_ms: 0.0,
                passed: 0,
                failed: 0,
                tests: vec![],
                error: Some(format!("auth error: {e}")),
            };
            let _ = app.emit(
                "runner:progress",
                RunnerProgress {
                    run_id: run_id.clone(),
                    request_id: saved.id.clone(),
                    request_name: saved.name.clone(),
                    index: idx,
                    total,
                    result: Some(result.clone()),
                },
            );
            return Ok(result);
        }
    }

    // Pre-script.
    let pre_result = if !saved.pre_request_script.trim().is_empty() {
        let ctx = PreScriptContext {
            method: req.method.clone(),
            url: req.url.clone(),
            headers: req.headers.iter().filter(|h| h.enabled)
                .map(|h| (h.name.clone(), h.value.clone()))
                .collect(),
            query: req.query.iter().filter(|q| q.enabled)
                .map(|q| (q.key.clone(), q.value.clone()))
                .collect(),
            env: resolved.values.clone(),
        };
        match run_pre_script(&saved.pre_request_script, &ctx) {
            Ok(r) => {
                for (k, v) in &r.env_mutations {
                    resolved.values.insert(k.clone(), v.clone());
                }
                if r.aborted {
                    let result = RequestRunResult {
                        status: None,
                        duration_ms: 0.0,
                        passed: r.passed(),
                        failed: r.failed(),
                        tests: r.tests,
                        error: Some("aborted by pre-request script".into()),
                    };
                    let _ = app.emit(
                        "runner:progress",
                        RunnerProgress {
                            run_id: run_id.clone(),
                            request_id: saved.id.clone(),
                            request_name: saved.name.clone(),
                            index: idx,
                            total,
                            result: Some(result.clone()),
                        },
                    );
                    return Ok(result);
                }
                Some(r)
            }
            Err(e) => {
                let result = RequestRunResult {
                    status: None,
                    duration_ms: 0.0,
                    passed: 0,
                    failed: 0,
                    tests: vec![],
                    error: Some(format!("pre-script error: {e}")),
                };
                let _ = app.emit(
                    "runner:progress",
                    RunnerProgress {
                        run_id: run_id.clone(),
                        request_id: saved.id.clone(),
                        request_name: saved.name.clone(),
                        index: idx,
                        total,
                        result: Some(result.clone()),
                    },
                );
                return Ok(result);
            }
        }
    } else {
        None
    };

    // Send.
    let send_result = crate::engine::http::send(req.clone(), None).await;

    // Post-script.
    let post_result = match &send_result {
        Ok(resp) if !saved.post_response_script.trim().is_empty() => {
            let body = base64::engine::general_purpose::STANDARD
                .decode(&resp.body_base64)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default();
            let ctx = PostScriptContext {
                method: req.method.clone(),
                url: req.url.clone(),
                request_headers: req.headers.iter().filter(|h| h.enabled)
                    .map(|h| (h.name.clone(), h.value.clone()))
                    .collect(),
                status: resp.status,
                status_text: resp.status_text.clone(),
                response_headers: resp.headers.iter()
                    .map(|h| (h.name.clone(), h.value.clone()))
                    .collect(),
                body,
                duration_ms: resp.timing.total_ms,
                env: resolved.values.clone(),
            };
            run_post_script(&saved.post_response_script, &ctx).ok()
        }
        _ => None,
    };

    let all_tests: Vec<TestResult> = pre_result
        .as_ref()
        .map(|r| r.tests.clone())
        .unwrap_or_default()
        .into_iter()
        .chain(
            post_result
                .as_ref()
                .map(|r| r.tests.clone())
                .unwrap_or_default()
                .into_iter(),
        )
        .collect();

    let result = match send_result {
        Ok(resp) => RequestRunResult {
            status: Some(resp.status),
            duration_ms: resp.timing.total_ms,
            passed: all_tests.iter().filter(|t| t.passed).count(),
            failed: all_tests.iter().filter(|t| !t.passed).count(),
            tests: all_tests,
            // Pre-script errors were being dropped here — only
            // post_result's was checked, so an uncaught exception in
            // a pre-request script (that didn't call pm.abort()) sent
            // silently, with the request still showing as passed.
            error: pre_result.as_ref().and_then(|r| r.error.clone())
                .or_else(|| post_result.as_ref().and_then(|r| r.error.clone())),
        },
        Err(e) => RequestRunResult {
            status: None,
            duration_ms: 0.0,
            passed: 0,
            failed: 0,
            tests: all_tests,
            error: Some(e.to_string()),
        },
    };

    let _ = app.emit(
        "runner:progress",
        RunnerProgress {
            run_id: run_id.clone(),
            request_id: saved.id.clone(),
            request_name: saved.name.clone(),
            index: idx,
            total,
            result: Some(result.clone()),
        },
    );
    Ok(result)
}

// ---------------------------------------------------------------------------
// JUnit XML export
// ---------------------------------------------------------------------------

fn build_junit_xml(
    run_id: &str,
    names: &[&str],
    results: &[RequestRunResult],
) -> String {
    let total: usize = results.iter().map(|r| r.tests.len()).sum();
    let failures: usize = results.iter().map(|r| r.failed).sum();
    let errors: usize = results.iter().filter(|r| r.error.is_some()).count();

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="{run_id}" tests="{total}" failures="{failures}" errors="{errors}">
"#
    );

    for (i, result) in results.iter().enumerate() {
        let suite_name = xml_escape(names.get(i).copied().unwrap_or("unknown"));
        let test_count = result.tests.len();
        let fail_count = result.failed;
        let time_s = result.duration_ms / 1000.0;

        xml.push_str(&format!(
            r#"  <testsuite name="{suite_name}" tests="{test_count}" failures="{fail_count}" time="{time_s:.3}">
"#
        ));

        for test in &result.tests {
            let classname = xml_escape(&suite_name);
            let tname = xml_escape(&test.name);
            xml.push_str(&format!(
                r#"    <testcase classname="{classname}" name="{tname}" time="{time_s:.3}">
"#
            ));
            if !test.passed {
                let msg = xml_escape(test.error.as_deref().unwrap_or("assertion failed"));
                xml.push_str(&format!(
                    r#"      <failure message="{msg}" type="AssertionError"/>
"#
                ));
            }
            xml.push_str("    </testcase>\n");
        }

        if let Some(err) = &result.error {
            xml.push_str(&format!(
                r#"    <error message="{}" type="Error"/>
"#,
                xml_escape(err)
            ));
        }

        xml.push_str("  </testsuite>\n");
    }

    xml.push_str("</testsuites>");
    xml
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ---------------------------------------------------------------------------
// Data-driven parsing
// ---------------------------------------------------------------------------

/// Parse a data string as either JSON array-of-objects or CSV with header row.
fn parse_data(
    data: Option<&str>,
) -> AppResult<Vec<std::collections::HashMap<String, String>>> {
    let Some(s) = data else {
        return Ok(vec![]);
    };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    // JSON path: starts with '['
    if trimmed.starts_with('[') {
        let rows: Vec<serde_json::Value> = serde_json::from_str(trimmed)?;
        let out = rows
            .into_iter()
            .filter_map(|v| {
                if let serde_json::Value::Object(map) = v {
                    Some(
                        map.into_iter()
                            .map(|(k, v)| (k, value_to_string(v)))
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .collect();
        return Ok(out);
    }
    // CSV path
    let mut rdr = csv::Reader::from_reader(trimmed.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?
        .iter()
        .map(str::to_string)
        .collect();
    let mut out = Vec::new();
    for record in rdr.records() {
        let record = record.map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        let row: std::collections::HashMap<String, String> = headers
            .iter()
            .zip(record.iter())
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        out.push(row);
    }
    Ok(out)
}

fn value_to_string(v: serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}
