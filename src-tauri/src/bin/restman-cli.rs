//! Headless collection runner — a newman/postman-CLI-style alternative to
//! the GUI's `CollectionRunner` panel, for CI and scripted use with no
//! display/window server needed. Full parity with the GUI runner's config
//! surface (iterations, delay, parallel, data-driven runs) and its JUnit
//! XML / JSON export, reusing the exact same execution core
//! (`restman_lib::runner`) so behavior can't drift between the two.
//!
//! Opens the same on-disk SQLite DB the desktop app uses (the same
//! app-data-dir plus identifier convention Tauri's own
//! `app.path().app_data_dir()` resolves to) unless `--db` overrides it.
//! Never creates a workspace/DB — if the target doesn't already exist,
//! that's a clear error, not a silent empty run.

use clap::Parser;
use restman_lib::error::{AppError, AppResult};
use restman_lib::model::{Collection, SavedRequest, Workspace};
use restman_lib::runner::{self, CollectionRunSummary, RunnerProgress};
use restman_lib::store::{self, AppState};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Must match `identifier` in `tauri.conf.json` — that's what Tauri's own
/// `app.path().app_data_dir()` joins onto the platform data dir (see
/// `tauri`'s vendored `src/path/desktop.rs`), and this CLI needs to land on
/// the exact same DB file the GUI does.
const APP_IDENTIFIER: &str = "com.restman.app";

#[derive(Parser)]
#[command(name = "restman-cli", version, about = "Headless Restman collection runner")]
struct Cli {
    /// Workspace name or id.
    #[arg(long)]
    workspace: String,
    /// Collection name or id (looked up within the workspace).
    #[arg(long)]
    collection: String,
    /// Number of iterations when no --data file is given.
    #[arg(long, default_value_t = 1)]
    iterations: usize,
    /// Delay between requests in milliseconds. Ignored with --parallel.
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,
    /// Run each iteration's requests concurrently, in waves.
    #[arg(long)]
    parallel: bool,
    /// Path to a CSV or JSON (array-of-objects) data file for data-driven runs.
    #[arg(long)]
    data: Option<PathBuf>,
    /// Write JUnit XML results here.
    #[arg(long)]
    junit: Option<PathBuf>,
    /// Write the full JSON results summary here.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Path to the SQLite DB file. Defaults to the same file the desktop
    /// app uses (platform app-data dir + "restman.db").
    #[arg(long)]
    db: Option<PathBuf>,
}

fn default_db_path() -> AppResult<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| AppError::Other("could not determine the platform data directory".into()))?
        .join(APP_IDENTIFIER);
    Ok(dir.join("restman.db"))
}

fn build_state(db_path: &std::path::Path) -> AppResult<AppState> {
    if !db_path.exists() {
        return Err(AppError::Other(format!(
            "no restman database found at {} — run the desktop app at least once, or pass --db",
            db_path.display()
        )));
    }
    let conn = store::db::open(db_path)?;
    Ok(AppState {
        db: Mutex::new(conn),
        cookie_jar: Arc::new(reqwest_cookie_store::CookieStoreMutex::new(
            reqwest_cookie_store::CookieStore::new(),
        )),
        streams: Arc::new(Mutex::new(HashMap::new())),
        mock_servers: Mutex::new(HashMap::new()),
        grpc_schema_cache: Mutex::new(HashMap::new()),
    })
}

fn resolve_workspace(state: &AppState, needle: &str) -> AppResult<Workspace> {
    let conn = state.db.lock().unwrap();
    let all = store::workspaces::list(&conn)?;
    resolve_by_id_or_name(all, needle, "workspace")
}

fn resolve_collection(state: &AppState, workspace_id: &str, needle: &str) -> AppResult<Collection> {
    let conn = state.db.lock().unwrap();
    let all = store::collections::list(&conn, workspace_id)?;
    resolve_by_id_or_name(all, needle, "collection")
}

trait NamedEntity {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
}
impl NamedEntity for Workspace {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
}
impl NamedEntity for Collection {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
}

fn resolve_by_id_or_name<T: NamedEntity>(all: Vec<T>, needle: &str, kind: &str) -> AppResult<T> {
    if let Some(exact) = all.iter().position(|e| e.id() == needle) {
        return Ok(all.into_iter().nth(exact).unwrap());
    }
    let matches: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, e)| e.name().eq_ignore_ascii_case(needle))
        .map(|(i, _)| i)
        .collect();
    match matches.len() {
        0 => {
            let available: Vec<&str> = all.iter().map(|e| e.name()).collect();
            Err(AppError::NotFound(format!(
                "no {kind} named or id'd {needle:?} — available: {}",
                available.join(", ")
            )))
        }
        1 => Ok(all.into_iter().nth(matches[0]).unwrap()),
        _ => {
            let ids: Vec<&str> = matches.iter().map(|&i| all[i].id()).collect();
            Err(AppError::Other(format!(
                "{kind} name {needle:?} is ambiguous — matching ids: {}",
                ids.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str, name: &str) -> Workspace {
        Workspace {
            id: id.to_string(),
            name: name.to_string(),
            created_at: 0,
            updated_at: 0,
            is_active: false,
        }
    }

    #[test]
    fn resolves_by_exact_id_even_when_name_would_also_match_something_else() {
        let all = vec![ws("id-1", "Alpha"), ws("id-2", "Beta")];
        let found = resolve_by_id_or_name(all, "id-2", "workspace").unwrap();
        assert_eq!(found.name, "Beta");
    }

    #[test]
    fn resolves_by_case_insensitive_name() {
        let all = vec![ws("id-1", "Alpha"), ws("id-2", "Beta")];
        let found = resolve_by_id_or_name(all, "ALPHA", "workspace").unwrap();
        assert_eq!(found.id, "id-1");
    }

    #[test]
    fn errors_cleanly_when_nothing_matches() {
        let all = vec![ws("id-1", "Alpha")];
        let err = resolve_by_id_or_name(all, "Gamma", "workspace").unwrap_err();
        assert!(err.to_string().contains("Alpha"));
    }

    #[test]
    fn errors_on_ambiguous_name_listing_every_matching_id() {
        let all = vec![ws("id-1", "Dup"), ws("id-2", "Dup")];
        let err = resolve_by_id_or_name(all, "dup", "workspace").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("id-1") && msg.contains("id-2"));
    }
}

fn print_progress(p: &RunnerProgress) {
    match &p.result {
        None => eprintln!("[{}/{}] {} …", p.index + 1, p.total, p.request_name),
        Some(r) => {
            let icon = if r.failed == 0 && r.error.is_none() { "ok" } else { "FAIL" };
            let status = r.status.map(|s| s.to_string()).unwrap_or_else(|| "-".into());
            let mut line = format!(
                "[{}/{}] {} — {icon} {status} ({}✓ {}✗, {:.0}ms)",
                p.index + 1,
                p.total,
                p.request_name,
                r.passed,
                r.failed,
                r.duration_ms
            );
            if let Some(err) = &r.error {
                line.push_str(&format!(" — {err}"));
            }
            eprintln!("{line}");
        }
    }
}

fn print_summary(summary: &CollectionRunSummary) {
    println!(
        "{} requests, {} passed / {} failed — {} tests ({} passed, {} failed) in {:.0}ms",
        summary.total_requests,
        summary.passed_requests,
        summary.failed_requests,
        summary.total_tests,
        summary.passed_tests,
        summary.failed_tests,
        summary.duration_ms
    );
}

/// Sequential loop: one request at a time, honoring `delay_ms`.
async fn run_sequential(
    state: &AppState,
    run_id: &str,
    workspace_id: &str,
    collection_id: &str,
    all_requests: &[SavedRequest],
    data_vars: &HashMap<String, String>,
    delay_ms: u64,
) -> AppResult<Vec<runner::RequestRunResult>> {
    let total = all_requests.len();
    let mut results = Vec::with_capacity(total);
    for (idx, saved) in all_requests.iter().enumerate() {
        print_progress(&RunnerProgress {
            run_id: run_id.to_string(),
            request_id: saved.id.clone(),
            request_name: saved.name.clone(),
            index: idx,
            total,
            result: None,
        });
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        let result =
            runner::execute_one_request(state, workspace_id, collection_id, saved, data_vars).await?;
        print_progress(&RunnerProgress {
            run_id: run_id.to_string(),
            request_id: saved.id.clone(),
            request_name: saved.name.clone(),
            index: idx,
            total,
            result: Some(result.clone()),
        });
        results.push(result);
    }
    Ok(results)
}

/// Wave-based parallel loop, mirroring the GUI runner's concurrency model
/// (up to `runner::MAX_PARALLEL` requests in flight per wave) but built on
/// plain `tokio::spawn` + `Arc<AppState>` instead of Tauri's `AppHandle`.
async fn run_parallel(
    state: Arc<AppState>,
    run_id: &str,
    workspace_id: &str,
    collection_id: &str,
    all_requests: &[SavedRequest],
    data_vars: &HashMap<String, String>,
) -> AppResult<Vec<runner::RequestRunResult>> {
    let total = all_requests.len();
    let indexed: Vec<(usize, &SavedRequest)> = all_requests.iter().enumerate().collect();
    let mut results = Vec::with_capacity(total);

    for chunk in indexed.chunks(runner::MAX_PARALLEL) {
        for &(idx, saved) in chunk {
            print_progress(&RunnerProgress {
                run_id: run_id.to_string(),
                request_id: saved.id.clone(),
                request_name: saved.name.clone(),
                index: idx,
                total,
                result: None,
            });
        }
        let mut handles = Vec::with_capacity(chunk.len());
        for &(_, saved) in chunk {
            let state = state.clone();
            let workspace_id = workspace_id.to_string();
            let collection_id = collection_id.to_string();
            let saved = saved.clone();
            let data_vars = data_vars.clone();
            handles.push(tokio::spawn(async move {
                runner::execute_one_request(&state, &workspace_id, &collection_id, &saved, &data_vars)
                    .await
            }));
        }
        for (&(idx, saved), handle) in chunk.iter().zip(handles) {
            let result = handle
                .await
                .map_err(|e| AppError::Other(format!("collection runner task failed: {e}")))??;
            print_progress(&RunnerProgress {
                run_id: run_id.to_string(),
                request_id: saved.id.clone(),
                request_name: saved.name.clone(),
                index: idx,
                total,
                result: Some(result.clone()),
            });
            results.push(result);
        }
    }
    Ok(results)
}

async fn run(cli: Cli) -> AppResult<i32> {
    let db_path = match &cli.db {
        Some(p) => p.clone(),
        None => default_db_path()?,
    };
    let state = Arc::new(build_state(&db_path)?);

    let workspace = resolve_workspace(&state, &cli.workspace)?;
    let collection = resolve_collection(&state, &workspace.id, &cli.collection)?;

    let data = match &cli.data {
        Some(path) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| AppError::Other(format!("reading --data {}: {e}", path.display())))?,
        ),
        None => None,
    };

    let all_requests = runner::load_requests(&state, &collection.id)?;
    let total = all_requests.len();
    let data_rows = runner::parse_data(data.as_deref())?;
    let iteration_count = if data_rows.is_empty() {
        cli.iterations.max(1)
    } else {
        data_rows.len()
    };

    let run_id = uuid::Uuid::new_v4().to_string();
    let start = std::time::Instant::now();
    let mut all_results = Vec::new();

    for iteration in 0..iteration_count {
        let data_vars = data_rows.get(iteration).cloned().unwrap_or_default();
        let mut iter_results = if cli.parallel {
            run_parallel(
                state.clone(),
                &run_id,
                &workspace.id,
                &collection.id,
                &all_requests,
                &data_vars,
            )
            .await?
        } else {
            run_sequential(
                &state,
                &run_id,
                &workspace.id,
                &collection.id,
                &all_requests,
                &data_vars,
                cli.delay_ms,
            )
            .await?
        };
        all_results.append(&mut iter_results);
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

    let summary = CollectionRunSummary {
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
    };

    print_summary(&summary);

    if let Some(path) = &cli.junit {
        std::fs::write(path, &summary.junit_xml)
            .map_err(|e| AppError::Other(format!("writing --junit {}: {e}", path.display())))?;
    }
    if let Some(path) = &cli.json {
        let text = serde_json::to_string_pretty(&summary)?;
        std::fs::write(path, text)
            .map_err(|e| AppError::Other(format!("writing --json {}: {e}", path.display())))?;
    }

    Ok(if failed_tests > 0 || failed_requests > 0 { 1 } else { 0 })
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}
