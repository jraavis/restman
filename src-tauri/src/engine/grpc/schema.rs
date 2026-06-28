//! Runtime `.proto` upload + compile via `protox`, building a
//! `prost_reflect::DescriptorPool`. Lands as task #27.
//!
//! ## Input shape: virtual filesystem, never the real one
//!
//! A real `.proto` schema is rarely one file — a service definition commonly
//! `import`s shared message files. So the input here is a map of
//! `path -> source text` (`ProtoFileSet`) rather than a single string: the
//! caller (eventually the Tauri command in #29) hands in everything the user
//! uploaded — a main file plus whatever local imports it needs — keyed by
//! the relative path the `import "..."` statements reference.
//!
//! `protox::Compiler` is driven through a custom `FileResolver`
//! (`VfsResolver`) instead of `protox::compile`'s filesystem-`includes` form.
//! This is the load-bearing choice: a resolver backed by an in-memory map can
//! only ever resolve names present in that map. There is no include
//! directory, no cwd, no `/` — so there is structurally no path that widens
//! resolution onto the real filesystem, let alone the network (and per the
//! 17c spike's `cargo tree` finding, protox/prost-reflect have no
//! HTTP-client-shaped dependency to begin with). This mirrors
//! `interop::openapi`'s local-only `$ref` resolution
//! (`resolve_ref`/`resolve_ref_depth` there): resolve only inside what the
//! caller handed in, fail closed on anything else, never fetch.
//!
//! `VfsResolver` is chained ahead of `protox::file::GoogleFileResolver` via
//! `ChainFileResolver` (`VfsResolver` first, so a user file can shadow a
//! well-known name if they really want to). Real `.proto` schemas commonly
//! `import "google/protobuf/timestamp.proto"` (or `duration`/`struct`/`any`/
//! `field_mask`/etc.) without shipping that file themselves — `protox::
//! compile`'s plain filesystem form gets these for free because `Compiler::
//! new` bundles a `GoogleFileResolver` automatically, but `with_file_resolver`
//! does not, so this module adds it back explicitly. This does not weaken
//! the no-real-filesystem/no-network posture: `GoogleFileResolver` serves
//! descriptors compiled into the `protox` binary itself, not files read from
//! disk or fetched over the wire.
//!
//! ## Output shape
//!
//! On success, returns a `prost_reflect::DescriptorPool` — the same type
//! `engine::grpc::reflection`'s server-reflection path builds
//! (`DescriptorPool::from_file_descriptor_set`, proven in 17c against the
//! vendored `reflection.proto` fixture). #30 (unary RPC) and friends can
//! consume a schema discovered either way through one shared type.
//!
//! ## Error handling
//!
//! Follows this repo's established convention for engine-layer code
//! (`engine::ws::connect` returns `Result<_, AppError>`, not a bespoke
//! per-module error enum or `anyhow`) — `crate::error::AppError`, with
//! `AppError::Other` carrying a formatted message. "Import not found" comes
//! back as a clean `Err` with the missing import's name and the file that
//! requested it, never a panic or a hang.

use crate::error::{AppError, AppResult};
use prost_reflect::DescriptorPool;
use protox::file::{ChainFileResolver, File as ProtoFile, FileResolver, GoogleFileResolver};
use protox::{Compiler, Error as ProtoxError};
use std::collections::BTreeMap;

/// User-supplied `.proto` files, keyed by the relative path their own
/// `import "..."` statements (and other files' imports of them) reference —
/// e.g. `"main.proto"` importing `"common/shared.proto"` means this map has
/// both keys, the second one matching the import string verbatim.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
pub(crate) type ProtoFileSet = BTreeMap<String, String>;

/// A `protox::file::FileResolver` backed entirely by an in-memory
/// `ProtoFileSet`. Never touches `std::fs`, so resolution can't widen onto
/// the real filesystem (or network — protox has no network-capable
/// transitive dependency per 17c) no matter what an import string says.
///
/// Owns a clone of the file set rather than borrowing it: `Compiler::
/// with_file_resolver` requires `R: 'static`, and the file sets handed to
/// this module are small (user-uploaded `.proto` text, not megabytes of
/// binary), so the clone is cheap relative to the compile work itself.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring)
struct VfsResolver {
    files: ProtoFileSet,
}

impl FileResolver for VfsResolver {
    fn open_file(&self, name: &str) -> Result<ProtoFile, ProtoxError> {
        match self.files.get(name) {
            Some(source) => ProtoFile::from_source(name, source),
            None => Err(ProtoxError::file_not_found(name)),
        }
    }
}

/// Compiles a user-supplied `.proto` file set and builds a queryable
/// `DescriptorPool` from it.
///
/// `entry_points` names the file(s) to compile (typically just the one main
/// file the user designated; any files it imports are pulled in from `files`
/// automatically). Every name must exist as a key in `files` — imports that
/// aren't in the set fail closed with `AppError::Other`, detailing the
/// missing path and, where protox reports it, the importing file.
///
/// No real filesystem path and no network address is ever consulted: the
/// only data this function can resolve a name against is `files` itself.
#[allow(dead_code)] // caller lands in #29 (Tauri command wiring) / #30 (unary RPC)
pub(crate) fn compile_proto_set(
    files: &ProtoFileSet,
    entry_points: &[String],
) -> AppResult<DescriptorPool> {
    if entry_points.is_empty() {
        return Err(AppError::Other(
            "no entry-point .proto file specified".into(),
        ));
    }

    let mut resolver = ChainFileResolver::new();
    resolver.add(VfsResolver {
        files: files.clone(),
    });
    resolver.add(GoogleFileResolver::new());
    let mut compiler = Compiler::with_file_resolver(resolver);
    compiler
        .include_imports(true)
        .open_files(entry_points)
        .map_err(describe_protox_error)?;

    let fds = compiler.file_descriptor_set();
    DescriptorPool::from_file_descriptor_set(fds).map_err(|e| {
        AppError::Other(format!(
            "compiled .proto file set produced an invalid descriptor pool: {e}"
        ))
    })
}

/// Turns a `protox::Error` into a clean, user-facing `AppError`, special-casing
/// "import not found" so the message names exactly what's missing rather than
/// surfacing protox's internal error formatting verbatim.
fn describe_protox_error(err: ProtoxError) -> AppError {
    if err.is_file_not_found() {
        // protox's own Display already reads "import '<name>' not found", so
        // this doesn't repeat that wording — it just adds the importer when
        // protox can tell us which file requested the missing import.
        return match err.file() {
            Some(importer) => {
                AppError::Other(format!("{err} (imported from \"{importer}\")"))
            }
            None => AppError::Other(err.to_string()),
        };
    }
    if err.is_parse() {
        return match err.file() {
            Some(file) => AppError::Other(format!("failed to parse \"{file}\": {err}")),
            None => AppError::Other(format!("failed to parse .proto file: {err}")),
        };
    }
    AppError::Other(format!("failed to compile .proto file set: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::grpc::testsupport::FIXTURES_ROOT;

    /// Reads a fixture `.proto` file from disk (test-only) so its contents
    /// can be fed into the in-memory `ProtoFileSet` this module's production
    /// code accepts — this is just how the *test* gets data, the function
    /// under test never touches the filesystem itself.
    fn read_fixture(relative_path: &str) -> String {
        std::fs::read_to_string(format!("{FIXTURES_ROOT}/{relative_path}"))
            .unwrap_or_else(|e| panic!("fixture {relative_path} should be readable: {e}"))
    }

    #[test]
    fn compiles_single_file_with_no_imports() {
        let mut files = ProtoFileSet::new();
        files.insert(
            "reflection.proto".to_string(),
            read_fixture("reflection.proto"),
        );

        let pool = compile_proto_set(&files, &["reflection.proto".to_string()])
            .expect("self-contained file with no imports should compile");

        assert!(pool
            .get_message_by_name("grpc.reflection.v1.ServerReflectionRequest")
            .is_some());
    }

    /// Locks the `ChainFileResolver` + `GoogleFileResolver` composition: a
    /// well-known import is resolved purely from descriptors compiled into
    /// `protox` itself — not from real disk (the file set below has nothing
    /// but the one main file) and not from the network (no network-capable
    /// dependency exists in this binary per 17c). Without the
    /// `GoogleFileResolver` fallback this would fail "import not found,"
    /// which would break a large fraction of real-world `.proto` schemas
    /// (`Timestamp`/`Duration`/`Any`/etc. are extremely common).
    #[test]
    fn well_known_google_import_resolves_without_being_in_the_set() {
        let mut files = ProtoFileSet::new();
        files.insert(
            "main.proto".to_string(),
            concat!(
                "syntax = \"proto3\";\n",
                "import \"google/protobuf/timestamp.proto\";\n",
                "message Event {\n",
                "  google.protobuf.Timestamp occurred_at = 1;\n",
                "}\n",
            )
            .to_string(),
        );

        let pool = compile_proto_set(&files, &["main.proto".to_string()]).expect(
            "a well-known google/protobuf import should resolve even though it's not in the file set",
        );

        let event = pool
            .get_message_by_name("Event")
            .expect("Event message should be present");
        let field = event
            .get_field_by_name("occurred_at")
            .expect("Event.occurred_at field should exist");
        assert_eq!(
            field.kind().as_message().map(|m| m.full_name().to_string()),
            Some("google.protobuf.Timestamp".to_string())
        );
    }

    #[test]
    fn compiles_main_file_with_local_import_present_in_the_set() {
        let mut files = ProtoFileSet::new();
        files.insert(
            "importer/main.proto".to_string(),
            read_fixture("importer/main.proto"),
        );
        files.insert(
            "common/shared.proto".to_string(),
            read_fixture("common/shared.proto"),
        );

        let pool = compile_proto_set(&files, &["importer/main.proto".to_string()])
            .expect("import present in the file set should resolve");

        let wrapper = pool
            .get_message_by_name("spike.importer.Wrapper")
            .expect("Wrapper message should be present");
        let inner_field = wrapper
            .get_field_by_name("inner")
            .expect("Wrapper.inner field should exist");
        assert_eq!(
            inner_field.kind().as_message().map(|m| m.full_name().to_string()),
            Some("spike.common.Shared".to_string())
        );
    }

    #[test]
    fn errors_cleanly_when_import_is_missing_from_the_file_set() {
        // Same importer/main.proto, but "common/shared.proto" is deliberately
        // left out of the set entirely — unlike 17c's filesystem-include-root
        // negative test, there's no real directory to narrow here at all,
        // proving the missing-import path fails closed for the in-memory
        // resolver too, not just for `protox::compile`'s filesystem form.
        let mut files = ProtoFileSet::new();
        files.insert(
            "importer/main.proto".to_string(),
            read_fixture("importer/main.proto"),
        );

        let err = compile_proto_set(&files, &["importer/main.proto".to_string()])
            .expect_err("missing import should fail closed, not hang or panic");

        let message = err.to_string();
        assert!(
            message.contains("not found"),
            "error should clearly say the import wasn't found, got: {message}"
        );
        assert!(
            message.contains("common/shared.proto"),
            "error should name the missing import, got: {message}"
        );
    }

    #[test]
    fn errors_cleanly_on_malformed_proto_source() {
        let mut files = ProtoFileSet::new();
        files.insert(
            "broken.proto".to_string(),
            "this is not valid protobuf syntax {{{".to_string(),
        );

        let err = compile_proto_set(&files, &["broken.proto".to_string()])
            .expect_err("malformed source should fail closed, not panic");

        assert!(
            err.to_string().contains("broken.proto"),
            "error should name the offending file"
        );
    }

    #[test]
    fn errors_when_no_entry_points_given() {
        let files = ProtoFileSet::new();
        let err = compile_proto_set(&files, &[]).expect_err("empty entry points should error");
        assert!(err.to_string().contains("no entry-point"));
    }

    #[test]
    fn entry_point_itself_missing_from_the_set_errors_cleanly() {
        let files = ProtoFileSet::new();
        let err = compile_proto_set(&files, &["does_not_exist.proto".to_string()])
            .expect_err("entry point absent from the set should fail closed");
        assert!(err.to_string().contains("does_not_exist.proto"));
    }

    /// Mirrors 17c's "import resolution confirmed local-only" finding, but at
    /// this module's own layer, and with a sharper negative case than "the
    /// import string is bogus": here `import "common/shared.proto"` names a
    /// real file that genuinely exists on disk under `FIXTURES_ROOT` — the
    /// same file `compiles_main_file_with_local_import_present_in_the_set`
    /// successfully resolves a few lines up — but it's deliberately left out
    /// of *this* test's in-memory `files` map. If `VfsResolver` ever fell
    /// back to the real filesystem (e.g. by accidentally constructing a
    /// `protox::Compiler` with filesystem include paths instead of the
    /// custom resolver), this would silently succeed instead of failing
    /// closed. It must still fail.
    #[test]
    fn import_present_on_real_disk_but_absent_from_the_set_is_not_found() {
        let mut files = ProtoFileSet::new();
        files.insert(
            "importer/main.proto".to_string(),
            read_fixture("importer/main.proto"),
        );
        // Deliberately NOT inserting "common/shared.proto", even though it
        // really exists at `{FIXTURES_ROOT}/common/shared.proto`.

        let err = compile_proto_set(&files, &["importer/main.proto".to_string()]).expect_err(
            "an import absent from the virtual file set must fail even when a same-named file exists on real disk",
        );
        let message = err.to_string();
        assert!(
            message.contains("not found"),
            "expected a clean file-not-found error, got: {message}"
        );
        assert!(
            message.contains("common/shared.proto"),
            "error should name the missing import, got: {message}"
        );
        assert!(
            message.contains("imported from \"importer/main.proto\""),
            "error should attribute the missing import to its importer, got: {message}"
        );
    }
}
