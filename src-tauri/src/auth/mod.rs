//! Auth: collection→request inheritance, keychain-backed secret
//! persistence, and applying a resolved `AuthConfig` to an outgoing request.
//!
//! OAuth2 token *lifecycle* (fetch/cache/refresh/the browser flow) lives in
//! `auth::oauth`; AWS SigV4 signing lives in `auth::aws_sigv4`. Both are
//! driven from here and from `commands/http.rs`, never from
//! `engine::http`, which stays DB-free — see module docs on `oauth`.

pub mod aws_sigv4;
pub mod oauth;

use crate::error::AppResult;
use crate::model::auth::{AuthConfig, RequestAuth};
use crate::model::variable::SECRET_MASK;
use crate::secrets;

/// Keychain namespace for an auth owner. `kind` is `"collection"` or
/// `"request"`; `id` is that row's own id (stable for its lifetime).
pub fn owner_key(kind: &str, id: &str) -> String {
    format!("auth:{kind}:{id}")
}

fn slot_key(owner: &str, slot: &str) -> String {
    format!("{owner}:{slot}")
}

/// Flat 2-tier resolution: a request's own auth wins if set, otherwise its
/// collection's. Deliberately does not walk up nested parent collections —
/// only the request's *direct* collection is consulted, mirroring
/// `VarScope::Collection` (see `crate::vars`). Returns the owner key the
/// resolved config's secrets are stored under, so the caller can `hydrate`.
pub fn resolve(collection_owner: Option<(&str, AuthConfig)>, request_auth: RequestAuth, request_id: &str) -> (String, AuthConfig) {
    match request_auth {
        RequestAuth::Own(cfg) => (owner_key("request", request_id), cfg),
        RequestAuth::Inherit => match collection_owner {
            Some((collection_id, cfg)) => (owner_key("collection", collection_id), cfg),
            None => (String::new(), AuthConfig::None),
        },
    }
}

/// Before storing `config` (in `collections.auth_json` / `requests.auth_json`):
/// write any new real secret values to the keychain and replace them with
/// `SECRET_MASK` in the returned config, so the DB never holds plaintext.
/// A field already equal to `SECRET_MASK` is left untouched (user didn't
/// touch it — same round-trip contract as `Variable`). A field set to `""`
/// clears that slot's keychain entry. Slots that belong to a *different*
/// variant than `config` (i.e. the auth type just changed) are swept too,
/// so switching Bearer → Basic doesn't leave an orphaned bearer-token entry.
pub fn persist(owner: &str, config: AuthConfig) -> AppResult<AuthConfig> {
    let mut config = config;
    let live_slots: Vec<&str> = config.secret_fields().iter().map(|(slot, _)| *slot).collect();
    for slot in AuthConfig::secret_slots() {
        if !live_slots.contains(slot) {
            let _ = secrets::delete(&slot_key(owner, slot));
        }
    }
    // Collected as owned data up front: `secret_fields()` borrows `config`,
    // and the loop body below needs to reassign `config` (`with_secret_field`
    // takes `self` by value), which the borrow checker won't allow while any
    // borrowed `&str` from that call is still in scope.
    let pending: Vec<(&'static str, String)> =
        config.secret_fields().into_iter().map(|(slot, value)| (slot, value.to_string())).collect();
    for (slot, value) in pending {
        if value == SECRET_MASK {
            continue; // untouched — keychain already holds the real value
        }
        if value.is_empty() {
            let _ = secrets::delete(&slot_key(owner, slot));
            continue;
        }
        secrets::set(&slot_key(owner, slot), &value)?;
        config = config.with_secret_field(slot, SECRET_MASK.to_string());
    }
    Ok(config)
}

/// `persist` for the request-level `RequestAuth` wrapper. `Inherit` has no
/// secrets of its own, so it passes through untouched (any stale secrets
/// from a previous `Own` are still swept, since `AuthConfig::None`'s
/// `secret_fields()` is empty — every known slot counts as "not live").
pub fn persist_request_auth(owner: &str, auth: RequestAuth) -> AppResult<RequestAuth> {
    Ok(match auth {
        RequestAuth::Inherit => {
            let _ = persist(owner, AuthConfig::None)?;
            RequestAuth::Inherit
        }
        RequestAuth::Own(cfg) => RequestAuth::Own(persist(owner, cfg)?),
    })
}

/// `hydrate` for the request-level `RequestAuth` wrapper — used when
/// duplicating a request (or a collection that owns one), to recover the
/// real secret under the old owner before `persist_request_auth` re-masks
/// and re-stores it under the new owner.
pub fn hydrate_request_auth(owner: &str, auth: RequestAuth) -> AppResult<RequestAuth> {
    Ok(match auth {
        RequestAuth::Inherit => RequestAuth::Inherit,
        RequestAuth::Own(cfg) => RequestAuth::Own(hydrate(owner, cfg)?),
    })
}

/// Recover real secret values from the keychain for fields still holding
/// `SECRET_MASK`. Used only in memory, right before a request is sent or a
/// duplicate is re-persisted under a new owner — never returned over IPC.
pub fn hydrate(owner: &str, config: AuthConfig) -> AppResult<AuthConfig> {
    let mut config = config;
    let masked: Vec<&'static str> =
        config.secret_fields().into_iter().filter(|(_, v)| *v == SECRET_MASK).map(|(slot, _)| slot).collect();
    for slot in masked {
        let real = secrets::get(&slot_key(owner, slot))?.unwrap_or_default();
        config = config.with_secret_field(slot, real);
    }
    Ok(config)
}

/// Masks every secret field in an already-hydrated `config` back to
/// `SECRET_MASK`, with no keychain I/O. For contexts that need a
/// safe-to-persist/display copy of a real config without writing (or
/// re-writing) anything to the keychain — e.g. the copy of a sent request
/// written to history, which must never carry the real bearer token/password/
/// API key/secret key that `resolve_auth` just hydrated for the actual send.
pub fn mask_secrets(config: AuthConfig) -> AuthConfig {
    let slots: Vec<&str> = config.secret_fields().iter().map(|(slot, _)| *slot).collect();
    let mut config = config;
    for slot in slots {
        config = config.with_secret_field(slot, SECRET_MASK.to_string());
    }
    config
}

/// Delete every secret slot for `owner`. Call when the owning collection or
/// request row is deleted, so its keychain entries don't outlive it.
pub fn sweep(owner: &str) {
    for slot in AuthConfig::secret_slots() {
        let _ = secrets::delete(&slot_key(owner, slot));
    }
    oauth::token_store::clear(owner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::auth::ApiKeyLocation;

    #[test]
    fn persist_masks_new_secret_and_hydrate_recovers_it() {
        let owner = owner_key("request", "req-1");
        let masked = persist(&owner, AuthConfig::Bearer { token: "real-token".into() }).unwrap();
        assert_eq!(masked, AuthConfig::Bearer { token: SECRET_MASK.into() });

        let real = hydrate(&owner, masked).unwrap();
        assert_eq!(real, AuthConfig::Bearer { token: "real-token".into() });
    }

    #[test]
    fn persist_leaves_mask_untouched_without_keychain_roundtrip() {
        let owner = owner_key("request", "req-2");
        persist(&owner, AuthConfig::Bearer { token: "first".into() }).unwrap();
        // Caller round-trips the mask (e.g. editor saved without touching
        // the field) — must not blow away the real stored value.
        let masked_again = persist(&owner, AuthConfig::Bearer { token: SECRET_MASK.into() }).unwrap();
        assert_eq!(masked_again, AuthConfig::Bearer { token: SECRET_MASK.into() });
        assert_eq!(hydrate(&owner, masked_again).unwrap(), AuthConfig::Bearer { token: "first".into() });
    }

    #[test]
    fn switching_auth_type_sweeps_previous_secret() {
        let owner = owner_key("collection", "col-1");
        persist(&owner, AuthConfig::Bearer { token: "tok".into() }).unwrap();
        persist(
            &owner,
            AuthConfig::ApiKey { key: "X-Key".into(), value: "v".into(), location: ApiKeyLocation::Header },
        )
        .unwrap();

        // The stale bearer-token slot must be gone, not just shadowed.
        assert_eq!(secrets::get(&slot_key(&owner, "bearer-token")).unwrap(), None);
    }

    #[test]
    fn empty_secret_field_clears_keychain_without_storing_mask() {
        let owner = owner_key("request", "req-3");
        persist(&owner, AuthConfig::Bearer { token: "tok".into() }).unwrap();
        let cleared = persist(&owner, AuthConfig::Bearer { token: String::new() }).unwrap();
        assert_eq!(cleared, AuthConfig::Bearer { token: String::new() });
        assert_eq!(secrets::get(&slot_key(&owner, "bearer-token")).unwrap(), None);
    }

    #[test]
    fn resolve_prefers_request_own_over_collection() {
        let cfg = AuthConfig::Bearer { token: SECRET_MASK.into() };
        let (owner, resolved) = resolve(
            Some(("col-1", AuthConfig::Basic { username: "u".into(), password: SECRET_MASK.into() })),
            RequestAuth::Own(cfg.clone()),
            "req-1",
        );
        assert_eq!(owner, owner_key("request", "req-1"));
        assert_eq!(resolved, cfg);
    }

    #[test]
    fn resolve_falls_through_to_collection_on_inherit() {
        let collection_cfg = AuthConfig::Basic { username: "u".into(), password: SECRET_MASK.into() };
        let (owner, resolved) = resolve(Some(("col-1", collection_cfg.clone())), RequestAuth::Inherit, "req-1");
        assert_eq!(owner, owner_key("collection", "col-1"));
        assert_eq!(resolved, collection_cfg);
    }
}
