//! IPC commands for the scripting / test-runner subsystem.
//!
//! The actual per-request execution, data parsing, and JUnit export live in
//! `crate::runner` — shared with the headless CLI runner
//! (`src/bin/restman-cli.rs`). This module is the Tauri-specific glue on top:
//! `AppHandle`-based progress events and `tauri::async_runtime::spawn` for
//! the parallel wave loop.

use crate::error::AppResult;
use crate::runner::{
    self, CollectionRunOptions, CollectionRunSummary, RequestRunResult, RunnerProgress,
};
use crate::store::AppState;
use tauri::{AppHandle, Emitter, Manager, State};

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
    let all_requests = runner::load_requests(state.inner(), &options.collection_id)?;
    let total = all_requests.len();

    // Parse data rows for data-driven runs.
    let data_rows: Vec<std::collections::HashMap<String, String>> =
        runner::parse_data(options.data.as_deref())?;
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
            let indexed: Vec<(usize, &crate::model::SavedRequest)> =
                all_requests.iter().enumerate().collect();
            for chunk in indexed.chunks(runner::MAX_PARALLEL) {
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
                    let result = handle.await.map_err(|e| {
                        crate::error::AppError::Other(format!("collection runner task failed: {e}"))
                    })??;
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

    let junit_xml = runner::build_junit_xml(
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

/// Runs one request via `crate::runner::execute_one_request`, emitting its
/// `runner:progress` start/finish events. Takes owned data and fetches
/// `AppState` from `app` itself (rather than a `State` extractor) so it can
/// also run standalone inside a task spawned by `tauri::async_runtime::spawn`.
async fn run_one_request(
    app: AppHandle,
    run_id: String,
    workspace_id: String,
    collection_id: String,
    saved: crate::model::SavedRequest,
    idx: usize,
    total: usize,
    delay_ms: u64,
    data_vars: std::collections::HashMap<String, String>,
) -> AppResult<RequestRunResult> {
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

    let result = runner::execute_one_request(
        state.inner(),
        &workspace_id,
        &collection_id,
        &saved,
        &data_vars,
    )
    .await?;

    let _ = app.emit(
        "runner:progress",
        RunnerProgress {
            run_id,
            request_id: saved.id.clone(),
            request_name: saved.name.clone(),
            index: idx,
            total,
            result: Some(result.clone()),
        },
    );
    Ok(result)
}
