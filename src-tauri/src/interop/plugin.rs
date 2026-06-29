//! Plugin-backed import/export. Same IR shape every native format's
//! `parse`/`export` dispatch uses (`super::parse`/`super::export`), but
//! routed through a sandboxed JS plugin (`plugins::runtime`) instead of a
//! compiled Rust parser/exporter.
//!
//! No new masking is needed here, unlike `codegen::plugin`: `export`'s input
//! (`ImportedNode`) comes from `super::collect`, which already reads
//! `auth_json` straight from the DB — already mask-on-write, per this
//! module's own doc comment. `parse`'s output goes through the same
//! `apply_import` pipeline as every native parser, which routes any auth it
//! finds through `crate::auth::persist` before it ever touches the DB. A
//! plugin parsing untrusted file content is no more privileged than the
//! native `postman`/`curl`/etc. parsers in that respect.

use serde::Deserialize;

use crate::error::AppResult;

use super::{ImportPreview, ImportedNode};

/// Wire shape a `parse` plugin returns: the IR tree plus any warnings (e.g.
/// "this field isn't supported, degraded to a default") — mirrors what a
/// native parser bundles into the `ImportPreview` it returns directly.
/// `stats` isn't part of this shape: `ImportPreview::new` derives folder/
/// request counts from `root` itself, so a plugin author never has to keep
/// a count in sync by hand.
#[derive(Debug, Deserialize)]
struct PluginParseResult {
    root: ImportedNode,
    #[serde(default)]
    warnings: Vec<String>,
}

/// Run an import plugin's `parse(content)` function. Expects a JS object
/// shaped `{ root: ImportedNode, warnings?: string[] }`.
pub fn parse(source: &str, content: &str) -> AppResult<ImportPreview> {
    let args = [serde_json::Value::String(content.to_string())];
    let result: PluginParseResult = crate::plugins::call_returning_json(source, "parse", &args)?;
    Ok(ImportPreview::new(result.root, result.warnings))
}

/// Run an export plugin's `exportCollection(node)` function. Expects a JS
/// string return — same contract as `codegen::plugin::generate`. Named
/// `exportCollection`, not `export`: `export` is an ECMAScript reserved word
/// and can't be used as a function name in any JS context, module or not.
pub fn export(source: &str, node: &ImportedNode) -> AppResult<String> {
    let args = [serde_json::to_value(node)?];
    crate::plugins::call_returning_string(source, "exportCollection", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plugin_round_trips_a_minimal_tree() {
        let preview = parse(
            r#"
            function parse(content) {
                return {
                    root: { name: "Imported", requests: [{ name: "Ping", method: "GET", url: content }] },
                    warnings: ["heads up"],
                };
            }
            "#,
            "https://example.com",
        )
        .unwrap();
        assert_eq!(preview.root.name, "Imported");
        assert_eq!(preview.root.requests.len(), 1);
        assert_eq!(preview.root.requests[0].url, "https://example.com");
        assert_eq!(preview.warnings, vec!["heads up".to_string()]);
        assert_eq!(preview.stats.requests, 1);
    }

    #[test]
    fn parse_plugin_defaults_warnings_to_empty() {
        let preview = parse(
            r#"function parse(content) { return { root: { name: "X" } }; }"#,
            "ignored",
        )
        .unwrap();
        assert!(preview.warnings.is_empty());
    }

    #[test]
    fn export_plugin_returns_a_string() {
        let node = ImportedNode { name: "Root".into(), ..Default::default() };
        let out =
            export(r#"function exportCollection(node) { return "exported:" + node.name; }"#, &node).unwrap();
        assert_eq!(out, "exported:Root");
    }
}
