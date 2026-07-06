//! Mock server + rule repository. Two tables (`mock_servers` one-to-many
//! `mock_rules`), same workspace-scoped-many-rows shape as `plugins`.

use crate::error::{AppError, AppResult};
use crate::model::http::HeaderEntry;
use crate::model::{BodyMatcher, MockMatcher, MockRule, MockRuleInput, MockServer, MockServerInput};
use crate::util::now_millis;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const SELECT_SERVER: &str = "SELECT id, workspace_id, name, port, created_at, updated_at FROM mock_servers";
const SELECT_RULE: &str = "SELECT id, mock_server_id, method, path_pattern, status, headers_json, body, delay_ms,
    sort_order, query_matchers_json, header_matchers_json, body_matcher_json FROM mock_rules";

fn row_to_server(r: &rusqlite::Row) -> rusqlite::Result<MockServer> {
    Ok(MockServer {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        name: r.get(2)?,
        port: r.get::<_, i64>(3)? as u16,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

/// Raw JSON blob columns fixed up into their typed fields by `finish_rule`.
struct RuleJsonColumns {
    headers_json: String,
    query_matchers_json: String,
    header_matchers_json: String,
    body_matcher_json: Option<String>,
}

fn row_to_rule(r: &rusqlite::Row) -> rusqlite::Result<(MockRule, RuleJsonColumns)> {
    Ok((
        MockRule {
            id: r.get(0)?,
            mock_server_id: r.get(1)?,
            method: r.get(2)?,
            path_pattern: r.get(3)?,
            status: r.get::<_, i64>(4)? as u16,
            headers: Vec::new(),       // placeholder, fixed up below
            body: r.get(6)?,
            delay_ms: r.get::<_, i64>(7)? as u64,
            sort_order: r.get(8)?,
            query_matchers: Vec::new(), // placeholder, fixed up below
            header_matchers: Vec::new(), // placeholder, fixed up below
            body_matcher: None,          // placeholder, fixed up below
        },
        RuleJsonColumns {
            headers_json: r.get(5)?,
            query_matchers_json: r.get(9)?,
            header_matchers_json: r.get(10)?,
            body_matcher_json: r.get(11)?,
        },
    ))
}

fn finish_rule(pair: rusqlite::Result<(MockRule, RuleJsonColumns)>) -> AppResult<MockRule> {
    let (mut rule, cols) = pair?;
    rule.headers = serde_json::from_str::<Vec<HeaderEntry>>(&cols.headers_json)
        .map_err(|e| AppError::Other(format!("corrupt mock rule headers: {e}")))?;
    rule.query_matchers = serde_json::from_str::<Vec<MockMatcher>>(&cols.query_matchers_json)
        .map_err(|e| AppError::Other(format!("corrupt mock rule query matchers: {e}")))?;
    rule.header_matchers = serde_json::from_str::<Vec<MockMatcher>>(&cols.header_matchers_json)
        .map_err(|e| AppError::Other(format!("corrupt mock rule header matchers: {e}")))?;
    rule.body_matcher = cols
        .body_matcher_json
        .map(|json| serde_json::from_str::<BodyMatcher>(&json))
        .transpose()
        .map_err(|e| AppError::Other(format!("corrupt mock rule body matcher: {e}")))?;
    Ok(rule)
}

pub fn list_servers(conn: &Connection, workspace_id: &str) -> AppResult<Vec<MockServer>> {
    let mut stmt = conn.prepare(&format!("{SELECT_SERVER} WHERE workspace_id = ?1 ORDER BY name ASC"))?;
    let rows = stmt.query_map(params![workspace_id], row_to_server)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_server(conn: &Connection, id: &str) -> AppResult<MockServer> {
    conn.query_row(&format!("{SELECT_SERVER} WHERE id = ?1"), params![id], row_to_server)
        .map_err(|_| AppError::NotFound(format!("mock server {id}")))
}

pub fn create_server(conn: &Connection, workspace_id: &str, input: &MockServerInput) -> AppResult<MockServer> {
    let now = now_millis();
    let server = MockServer {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        name: input.name.clone(),
        port: input.port,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO mock_servers (id, workspace_id, name, port, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![server.id, server.workspace_id, server.name, server.port as i64, server.created_at],
    )?;
    Ok(server)
}

pub fn update_server(conn: &Connection, id: &str, input: &MockServerInput) -> AppResult<MockServer> {
    let n = conn.execute(
        "UPDATE mock_servers SET name = ?2, port = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, input.name, input.port as i64, now_millis()],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("mock server {id}")));
    }
    get_server(conn, id)
}

pub fn delete_server(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM mock_servers WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("mock server {id}")));
    }
    Ok(())
}

pub fn list_rules(conn: &Connection, mock_server_id: &str) -> AppResult<Vec<MockRule>> {
    let mut stmt =
        conn.prepare(&format!("{SELECT_RULE} WHERE mock_server_id = ?1 ORDER BY sort_order ASC, rowid ASC"))?;
    let rows = stmt.query_map(params![mock_server_id], row_to_rule)?;
    rows.map(finish_rule).collect::<AppResult<Vec<_>>>()
}

pub fn get_rule(conn: &Connection, id: &str) -> AppResult<MockRule> {
    let pair = conn
        .query_row(&format!("{SELECT_RULE} WHERE id = ?1"), params![id], row_to_rule)
        .map_err(|_| AppError::NotFound(format!("mock rule {id}")))?;
    finish_rule(Ok(pair))
}

/// Serializes a rule input's matcher fields to their JSON column values.
fn serialize_matchers(input: &MockRuleInput) -> AppResult<(String, String, String, Option<String>)> {
    let headers_json = serde_json::to_string(&input.headers)
        .map_err(|e| AppError::Other(format!("failed to serialize mock rule headers: {e}")))?;
    let query_matchers_json = serde_json::to_string(&input.query_matchers)
        .map_err(|e| AppError::Other(format!("failed to serialize mock rule query matchers: {e}")))?;
    let header_matchers_json = serde_json::to_string(&input.header_matchers)
        .map_err(|e| AppError::Other(format!("failed to serialize mock rule header matchers: {e}")))?;
    let body_matcher_json = input
        .body_matcher
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| AppError::Other(format!("failed to serialize mock rule body matcher: {e}")))?;
    Ok((headers_json, query_matchers_json, header_matchers_json, body_matcher_json))
}

pub fn create_rule(conn: &Connection, mock_server_id: &str, input: &MockRuleInput) -> AppResult<MockRule> {
    let rule = MockRule {
        id: Uuid::new_v4().to_string(),
        mock_server_id: mock_server_id.to_string(),
        method: input.method.clone(),
        path_pattern: input.path_pattern.clone(),
        status: input.status,
        headers: input.headers.clone(),
        body: input.body.clone(),
        delay_ms: input.delay_ms,
        sort_order: input.sort_order,
        query_matchers: input.query_matchers.clone(),
        header_matchers: input.header_matchers.clone(),
        body_matcher: input.body_matcher.clone(),
    };
    let (headers_json, query_matchers_json, header_matchers_json, body_matcher_json) = serialize_matchers(input)?;
    conn.execute(
        "INSERT INTO mock_rules (id, mock_server_id, method, path_pattern, status, headers_json, body, delay_ms,
             sort_order, query_matchers_json, header_matchers_json, body_matcher_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            rule.id,
            rule.mock_server_id,
            rule.method,
            rule.path_pattern,
            rule.status as i64,
            headers_json,
            rule.body,
            rule.delay_ms as i64,
            rule.sort_order,
            query_matchers_json,
            header_matchers_json,
            body_matcher_json,
        ],
    )?;
    Ok(rule)
}

pub fn update_rule(conn: &Connection, id: &str, input: &MockRuleInput) -> AppResult<MockRule> {
    let (headers_json, query_matchers_json, header_matchers_json, body_matcher_json) = serialize_matchers(input)?;
    let n = conn.execute(
        "UPDATE mock_rules SET method = ?2, path_pattern = ?3, status = ?4, headers_json = ?5, body = ?6,
             delay_ms = ?7, sort_order = ?8, query_matchers_json = ?9, header_matchers_json = ?10,
             body_matcher_json = ?11
         WHERE id = ?1",
        params![
            id,
            input.method,
            input.path_pattern,
            input.status as i64,
            headers_json,
            input.body,
            input.delay_ms as i64,
            input.sort_order,
            query_matchers_json,
            header_matchers_json,
            body_matcher_json,
        ],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("mock rule {id}")));
    }
    get_rule(conn, id)
}

pub fn delete_rule(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute("DELETE FROM mock_rules WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("mock rule {id}")));
    }
    Ok(())
}

/// Naive path+query extraction from a saved request's URL — good enough for
/// seeding a mock rule's `path_pattern`, not a general-purpose URL parser.
/// Deliberately doesn't pull in the `url` crate as a new production
/// dependency for this one call site; `{{var}}`-templated URLs (common in
/// this codebase) aren't valid URLs anyway, so a real parser wouldn't help.
fn path_from_url(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let path_and_query = match after_scheme.find('/') {
        Some(idx) => &after_scheme[idx..],
        None => "/",
    };
    path_and_query.split('?').next().unwrap_or("/").to_string()
}

/// Creates a mock server pre-populated with one rule per request in
/// `collection_id` (method + path extracted from the saved request, a
/// generic 200 stub response — this seeds a starting point, not a replay of
/// real traffic; wiring in a request's actual last-known response is a
/// possible follow-up, not attempted here to avoid coupling this feature to
/// history's data shape).
pub fn create_from_collection(
    conn: &Connection,
    workspace_id: &str,
    collection_id: &str,
    name: &str,
    port: u16,
) -> AppResult<MockServer> {
    let server = create_server(conn, workspace_id, &MockServerInput { name: name.to_string(), port })?;
    let requests = crate::store::requests::list_by_collection(conn, collection_id)?;
    for (i, req) in requests.iter().enumerate() {
        create_rule(
            conn,
            &server.id,
            &MockRuleInput {
                method: Some(req.method.clone()),
                path_pattern: path_from_url(&req.url),
                status: 200,
                headers: Vec::new(),
                body: String::new(),
                delay_ms: 0,
                sort_order: i as i64,
                query_matchers: Vec::new(),
                header_matchers: Vec::new(),
                body_matcher: None,
            },
        )?;
    }
    Ok(server)
}

/// A mock server's shareable/backup-able config — the server's name+port
/// plus its full ordered rule list, as `MockRuleInput` (no ids/timestamps,
/// same "config only, no identity" shape `MockServerInput` already has).
/// No secrets live in a mock rule's fields, unlike environment/collection
/// export, so this needs no masking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MockServerExport {
    name: String,
    port: u16,
    rules: Vec<MockRuleInput>,
}

/// Serializes a mock server (and its rules) to a pretty-printed JSON string
/// suitable for saving to disk and re-importing via `import_server`.
pub fn export_server(conn: &Connection, id: &str) -> AppResult<String> {
    let server = get_server(conn, id)?;
    let rules = list_rules(conn, id)?;
    let export = MockServerExport {
        name: server.name,
        port: server.port,
        rules: rules
            .into_iter()
            .map(|r| MockRuleInput {
                method: r.method,
                path_pattern: r.path_pattern,
                status: r.status,
                headers: r.headers,
                body: r.body,
                delay_ms: r.delay_ms,
                sort_order: r.sort_order,
                query_matchers: r.query_matchers,
                header_matchers: r.header_matchers,
                body_matcher: r.body_matcher,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&export).map_err(|e| AppError::Other(format!("failed to serialize mock server export: {e}")))
}

/// Creates a new mock server (and its rules) from a previously exported
/// JSON string. `sort_order` is overwritten with the file's rule order
/// (rather than trusted verbatim) so a hand-edited export can't produce a
/// conflicting/duplicate ordering.
pub fn import_server(conn: &Connection, workspace_id: &str, content: &str) -> AppResult<MockServer> {
    let export: MockServerExport =
        serde_json::from_str(content).map_err(|e| AppError::Other(format!("invalid mock server export: {e}")))?;
    let server = create_server(conn, workspace_id, &MockServerInput { name: export.name, port: export.port })?;
    for (i, mut rule) in export.rules.into_iter().enumerate() {
        rule.sort_order = i as i64;
        create_rule(conn, &server.id, &rule)?;
    }
    Ok(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_with_workspace() -> (Connection, String) {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let ws = crate::store::workspaces::ensure_default(&mut conn).unwrap();
        (conn, ws.id)
    }

    fn sample_server_input(name: &str) -> MockServerInput {
        MockServerInput { name: name.to_string(), port: 3001 }
    }

    fn sample_rule_input(path: &str) -> MockRuleInput {
        MockRuleInput {
            method: Some("GET".into()),
            path_pattern: path.to_string(),
            status: 200,
            headers: vec![HeaderEntry { name: "Content-Type".into(), value: "application/json".into(), enabled: true }],
            body: "{}".into(),
            delay_ms: 0,
            sort_order: 0,
            query_matchers: Vec::new(),
            header_matchers: Vec::new(),
            body_matcher: None,
        }
    }

    /// A rule input exercising every new matcher field with non-default
    /// values — a round-trip test using only `sample_rule_input`'s all-empty
    /// matchers would pass even if a column were silently dropped.
    fn matcher_heavy_rule_input(path: &str) -> MockRuleInput {
        MockRuleInput {
            method: Some("POST".into()),
            path_pattern: path.to_string(),
            status: 201,
            headers: Vec::new(),
            body: "{}".into(),
            delay_ms: 0,
            sort_order: 0,
            query_matchers: vec![MockMatcher { name: "env".into(), value: "prod".into(), enabled: true }],
            header_matchers: vec![MockMatcher { name: "x-key".into(), value: "secret".into(), enabled: false }],
            body_matcher: Some(crate::model::BodyMatcher {
                mode: crate::model::BodyMatchMode::JsonEquals,
                json_path: "action".into(),
                value: "create".into(),
            }),
        }
    }

    #[test]
    fn create_server_then_get_round_trips() {
        let (conn, ws) = mem_with_workspace();
        let created = create_server(&conn, &ws, &sample_server_input("My Mock")).unwrap();
        assert_eq!(created.workspace_id, ws);
        assert_eq!(created.port, 3001);

        let fetched = get_server(&conn, &created.id).unwrap();
        assert_eq!(fetched.name, "My Mock");
    }

    #[test]
    fn list_servers_excludes_other_workspaces() {
        let (conn, ws1) = mem_with_workspace();
        create_server(&conn, &ws1, &sample_server_input("A")).unwrap();
        let ws2 = crate::store::workspaces::create(&conn, "Other").unwrap();
        create_server(&conn, &ws2.id, &sample_server_input("B")).unwrap();

        assert_eq!(list_servers(&conn, &ws1).unwrap().len(), 1);
        assert_eq!(list_servers(&conn, &ws2.id).unwrap().len(), 1);
    }

    #[test]
    fn update_server_changes_fields() {
        let (conn, ws) = mem_with_workspace();
        let created = create_server(&conn, &ws, &sample_server_input("Original")).unwrap();
        let updated = update_server(&conn, &created.id, &MockServerInput { name: "Renamed".into(), port: 4000 }).unwrap();
        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.port, 4000);
    }

    #[test]
    fn delete_server_cascades_to_rules() {
        let (conn, ws) = mem_with_workspace();
        let server = create_server(&conn, &ws, &sample_server_input("Doomed")).unwrap();
        create_rule(&conn, &server.id, &sample_rule_input("/x")).unwrap();
        delete_server(&conn, &server.id).unwrap();

        assert!(matches!(get_server(&conn, &server.id), Err(AppError::NotFound(_))));
        assert_eq!(list_rules(&conn, &server.id).unwrap().len(), 0);
    }

    #[test]
    fn create_rule_round_trips_headers() {
        let (conn, ws) = mem_with_workspace();
        let server = create_server(&conn, &ws, &sample_server_input("S")).unwrap();
        let created = create_rule(&conn, &server.id, &sample_rule_input("/users/:id")).unwrap();

        let fetched = get_rule(&conn, &created.id).unwrap();
        assert_eq!(fetched.path_pattern, "/users/:id");
        assert_eq!(fetched.headers.len(), 1);
        assert_eq!(fetched.headers[0].name, "Content-Type");
    }

    #[test]
    fn list_rules_ordered_by_sort_order() {
        let (conn, ws) = mem_with_workspace();
        let server = create_server(&conn, &ws, &sample_server_input("S")).unwrap();
        let mut third = sample_rule_input("/c");
        third.sort_order = 2;
        let mut first = sample_rule_input("/a");
        first.sort_order = 0;
        let mut second = sample_rule_input("/b");
        second.sort_order = 1;
        create_rule(&conn, &server.id, &third).unwrap();
        create_rule(&conn, &server.id, &first).unwrap();
        create_rule(&conn, &server.id, &second).unwrap();

        let rules = list_rules(&conn, &server.id).unwrap();
        let paths: Vec<&str> = rules.iter().map(|r| r.path_pattern.as_str()).collect();
        assert_eq!(paths, vec!["/a", "/b", "/c"]);
    }

    #[test]
    fn update_rule_changes_fields() {
        let (conn, ws) = mem_with_workspace();
        let server = create_server(&conn, &ws, &sample_server_input("S")).unwrap();
        let created = create_rule(&conn, &server.id, &sample_rule_input("/x")).unwrap();

        let mut updated_input = sample_rule_input("/y");
        updated_input.status = 404;
        let updated = update_rule(&conn, &created.id, &updated_input).unwrap();
        assert_eq!(updated.path_pattern, "/y");
        assert_eq!(updated.status, 404);
    }

    #[test]
    fn delete_rule_removes_the_row() {
        let (conn, ws) = mem_with_workspace();
        let server = create_server(&conn, &ws, &sample_server_input("S")).unwrap();
        let created = create_rule(&conn, &server.id, &sample_rule_input("/x")).unwrap();
        delete_rule(&conn, &created.id).unwrap();
        assert!(matches!(get_rule(&conn, &created.id), Err(AppError::NotFound(_))));
    }

    #[test]
    fn path_from_url_strips_scheme_host_and_query() {
        assert_eq!(path_from_url("https://api.example.com/users/42?x=1"), "/users/42");
        assert_eq!(path_from_url("https://api.example.com"), "/");
        assert_eq!(path_from_url("/already/a/path"), "/already/a/path");
        assert_eq!(path_from_url("https://api.example.com/users/{{id}}"), "/users/{{id}}");
    }

    #[test]
    fn create_from_collection_seeds_one_rule_per_request() {
        let (conn, ws) = mem_with_workspace();
        let collection = crate::store::collections::create(&conn, &ws, None, "My Collection", None).unwrap();
        crate::store::requests::create(
            &conn,
            &collection.id,
            &crate::model::SavedRequestInput {
                name: "Get user".into(),
                method: "GET".into(),
                url: "https://api.example.com/users/42".into(),
                headers: Vec::new(),
                query: Vec::new(),
                body: crate::model::http::RequestBody::None,
                options: crate::model::http::RequestOptions::default(),
                auth: Default::default(),
                pre_request_script: String::new(),
                post_response_script: String::new(),
                kind: Default::default(),
                stream_config: None,
            },
        )
        .unwrap();

        let server = create_from_collection(&conn, &ws, &collection.id, "From Collection", 3002).unwrap();
        let rules = list_rules(&conn, &server.id).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].method.as_deref(), Some("GET"));
        assert_eq!(rules[0].path_pattern, "/users/42");
    }

    /// A round-trip using only all-empty/default matcher fields would pass
    /// even if a column were silently dropped by `SELECT_RULE`/`create_rule`
    /// — this populates every new matcher field with a non-default value.
    #[test]
    fn create_rule_round_trips_matchers() {
        let (conn, ws) = mem_with_workspace();
        let server = create_server(&conn, &ws, &sample_server_input("S")).unwrap();
        let created = create_rule(&conn, &server.id, &matcher_heavy_rule_input("/orders")).unwrap();

        let fetched = get_rule(&conn, &created.id).unwrap();
        assert_eq!(fetched.query_matchers, vec![MockMatcher { name: "env".into(), value: "prod".into(), enabled: true }]);
        assert_eq!(
            fetched.header_matchers,
            vec![MockMatcher { name: "x-key".into(), value: "secret".into(), enabled: false }]
        );
        assert_eq!(
            fetched.body_matcher,
            Some(crate::model::BodyMatcher {
                mode: crate::model::BodyMatchMode::JsonEquals,
                json_path: "action".into(),
                value: "create".into(),
            })
        );
    }

    #[test]
    fn update_rule_round_trips_matchers() {
        let (conn, ws) = mem_with_workspace();
        let server = create_server(&conn, &ws, &sample_server_input("S")).unwrap();
        let created = create_rule(&conn, &server.id, &sample_rule_input("/x")).unwrap();

        let updated = update_rule(&conn, &created.id, &matcher_heavy_rule_input("/x")).unwrap();
        assert_eq!(updated.query_matchers.len(), 1);
        assert_eq!(updated.header_matchers.len(), 1);
        assert!(updated.body_matcher.is_some());
    }

    #[test]
    fn export_then_import_round_trips_server_and_matcher_heavy_rules() {
        let (conn, ws1) = mem_with_workspace();
        let server = create_server(&conn, &ws1, &sample_server_input("Exported")).unwrap();
        create_rule(&conn, &server.id, &sample_rule_input("/plain")).unwrap();
        create_rule(&conn, &server.id, &matcher_heavy_rule_input("/orders")).unwrap();

        let exported = export_server(&conn, &server.id).unwrap();

        let ws2 = crate::store::workspaces::create(&conn, "Other").unwrap();
        let imported = import_server(&conn, &ws2.id, &exported).unwrap();
        assert_eq!(imported.name, "Exported");
        assert_eq!(imported.port, 3001);
        assert_eq!(imported.workspace_id, ws2.id);

        let rules = list_rules(&conn, &imported.id).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].path_pattern, "/plain");
        assert_eq!(rules[1].path_pattern, "/orders");
        assert_eq!(rules[1].query_matchers, vec![MockMatcher { name: "env".into(), value: "prod".into(), enabled: true }]);
        assert_eq!(
            rules[1].body_matcher,
            Some(crate::model::BodyMatcher {
                mode: crate::model::BodyMatchMode::JsonEquals,
                json_path: "action".into(),
                value: "create".into(),
            })
        );
    }

    #[test]
    fn import_server_rejects_invalid_json() {
        let (conn, ws) = mem_with_workspace();
        assert!(matches!(import_server(&conn, &ws, "not json"), Err(AppError::Other(_))));
    }
}
