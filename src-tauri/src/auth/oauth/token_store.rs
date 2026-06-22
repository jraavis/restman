//! Cache for fetched OAuth2 tokens: small metadata in `oauth_tokens`
//! (token_type/scope/expiry/has_refresh_token), actual token strings in the
//! keychain under the same owner namespace as the rest of `auth` —
//! `auth:{kind}:{id}:token:{access|refresh}`, disjoint from the slot-based
//! keys `persist`/`hydrate` use, so neither sweep collides with the other.
//!
//! `clear` only touches the keychain: the `oauth_tokens` row's FK to
//! `collections`/`requests` is `ON DELETE CASCADE`, so the row itself is
//! already gone by the time `auth::sweep` runs.

use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct CachedToken {
    pub access_token: String,
    pub token_type: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub scope: Option<String>,
}

/// `owner` is `"auth:{kind}:{id}"` (see `auth::owner_key`). Returns `None`
/// for anything malformed rather than erroring — callers treat a parse
/// failure the same as a cache miss.
fn parse_owner(owner: &str) -> Option<(&str, &str)> {
    let mut parts = owner.splitn(3, ':');
    let prefix = parts.next()?;
    let kind = parts.next()?;
    let id = parts.next()?;
    if prefix != "auth" || id.is_empty() {
        return None;
    }
    Some((kind, id))
}

fn token_key(owner: &str, part: &str) -> String {
    format!("{owner}:token:{part}")
}

fn fk_column(kind: &str) -> AppResult<&'static str> {
    match kind {
        "collection" => Ok("collection_id"),
        "request" => Ok("request_id"),
        other => Err(AppError::Other(format!("unknown auth owner kind: {other}"))),
    }
}

/// `expires_at` is checked with a 30s skew buffer so a token doesn't expire
/// mid-flight between this check and the request actually going out.
pub fn is_fresh(token: &CachedToken) -> bool {
    match token.expires_at {
        Some(exp) => exp - 30_000 > crate::util::now_millis(),
        None => true,
    }
}

pub fn get(conn: &Connection, owner: &str) -> AppResult<Option<CachedToken>> {
    let Some((kind, id)) = parse_owner(owner) else {
        return Ok(None);
    };
    let column = fk_column(kind)?;
    let row: Option<(String, Option<String>, Option<i64>, i64)> = conn
        .query_row(
            &format!("SELECT token_type, scope, expires_at, has_refresh_token FROM oauth_tokens WHERE {column} = ?1"),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((token_type, scope, expires_at, has_refresh_token)) = row else {
        return Ok(None);
    };
    // Metadata row exists but the keychain entry is gone (e.g. user wiped
    // their keychain out-of-band) — treat as a miss rather than erroring.
    let Some(access_token) = crate::secrets::get(&token_key(owner, "access"))? else {
        return Ok(None);
    };
    let refresh_token = if has_refresh_token != 0 { crate::secrets::get(&token_key(owner, "refresh"))? } else { None };
    Ok(Some(CachedToken { access_token, token_type, refresh_token, expires_at, scope }))
}

/// Delete-then-insert: at most one token row per owner.
pub fn put(conn: &Connection, owner: &str, token: &CachedToken) -> AppResult<()> {
    let (kind, id) = parse_owner(owner).ok_or_else(|| AppError::Other(format!("malformed auth owner key: {owner}")))?;
    let column = fk_column(kind)?;
    conn.execute(&format!("DELETE FROM oauth_tokens WHERE {column} = ?1"), [id])?;
    let now = crate::util::now_millis();
    conn.execute(
        &format!(
            "INSERT INTO oauth_tokens (id, {column}, token_type, scope, expires_at, has_refresh_token, obtained_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        ),
        rusqlite::params![Uuid::new_v4().to_string(), id, token.token_type, token.scope, token.expires_at, token.refresh_token.is_some() as i64, now, now],
    )?;
    crate::secrets::set(&token_key(owner, "access"), &token.access_token)?;
    match &token.refresh_token {
        Some(rt) => crate::secrets::set(&token_key(owner, "refresh"), rt)?,
        None => crate::secrets::delete(&token_key(owner, "refresh"))?,
    }
    Ok(())
}

/// Keychain-only — see module docs for why the DB row needs no cleanup here.
pub fn clear(owner: &str) {
    let _ = crate::secrets::delete(&token_key(owner, "access"));
    let _ = crate::secrets::delete(&token_key(owner, "refresh"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner_with_collection(conn: &mut Connection) -> String {
        let ws = crate::store::workspaces::ensure_default(conn).unwrap();
        let c = crate::store::collections::create(conn, &ws.id, None, "Test", None).unwrap();
        crate::auth::owner_key("collection", &c.id)
    }

    #[test]
    fn put_then_get_roundtrips_through_keychain() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let owner = owner_with_collection(&mut conn);
        let token = CachedToken {
            access_token: "at-1".into(),
            token_type: "Bearer".into(),
            refresh_token: Some("rt-1".into()),
            expires_at: Some(crate::util::now_millis() + 3_600_000),
            scope: Some("read write".into()),
        };
        put(&conn, &owner, &token).unwrap();
        let fetched = get(&conn, &owner).unwrap().unwrap();
        assert_eq!(fetched, token);
        assert!(is_fresh(&fetched));
    }

    #[test]
    fn put_twice_replaces_rather_than_duplicates() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let owner = owner_with_collection(&mut conn);
        let first = CachedToken { access_token: "at-1".into(), token_type: "Bearer".into(), refresh_token: None, expires_at: None, scope: None };
        let second = CachedToken { access_token: "at-2".into(), token_type: "Bearer".into(), refresh_token: None, expires_at: None, scope: None };
        put(&conn, &owner, &first).unwrap();
        put(&conn, &owner, &second).unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM oauth_tokens", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
        assert_eq!(get(&conn, &owner).unwrap().unwrap().access_token, "at-2");
    }

    #[test]
    fn missing_expiry_counts_as_fresh() {
        let token = CachedToken { access_token: "at".into(), token_type: "Bearer".into(), refresh_token: None, expires_at: None, scope: None };
        assert!(is_fresh(&token));
    }

    #[test]
    fn expired_token_is_not_fresh() {
        let token = CachedToken {
            access_token: "at".into(),
            token_type: "Bearer".into(),
            refresh_token: None,
            expires_at: Some(crate::util::now_millis() - 1_000),
            scope: None,
        };
        assert!(!is_fresh(&token));
    }

    #[test]
    fn clear_removes_keychain_entries_but_get_still_needs_row_gone() {
        let mut conn = crate::store::db::open_in_memory().unwrap();
        let owner = owner_with_collection(&mut conn);
        let token = CachedToken { access_token: "at".into(), token_type: "Bearer".into(), refresh_token: Some("rt".into()), expires_at: None, scope: None };
        put(&conn, &owner, &token).unwrap();
        clear(&owner);
        // Row still exists (clear is keychain-only), but the access-token
        // keychain entry is gone, so get() reports a miss.
        assert_eq!(get(&conn, &owner).unwrap(), None);
    }
}
