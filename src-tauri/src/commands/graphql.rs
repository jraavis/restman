//! GraphQL schema introspection. A genuine live network fetch (real OAuth2
//! exchange, real proxy/mTLS transport) but deliberately not a `send_request`
//! call: introspection is a diagnostic fetch triggered by a "Fetch schema"
//! button, not a user-initiated send, so it must skip pre/post scripts,
//! history persistence, and `touch_last_used` — none of which make sense for
//! a query the user never wrote. Reuses `send_request`'s auth/vars resolution
//! helpers and calls `engine::http::send` directly, the same "resolve like a
//! real send, but don't persist" posture `commands::codegen::generate_code`
//! already established (see its module doc) — the difference here is
//! introspection needs the *real* OAuth2 token, not a cached-or-placeholder
//! one, since the response has to reflect a real server.

use crate::commands::http::resolve_auth;
use crate::error::{AppError, AppResult};
use crate::model::http::{HttpRequest, RequestBody};
use crate::store::AppState;
use base64::Engine as _;
use std::sync::Arc;
use tauri::State;

/// The standard GraphQL introspection query (from the GraphQL spec / graphql-js's
/// `getIntrospectionQuery()`), deep enough for `buildClientSchema` to reconstruct
/// a full `GraphQLSchema` client-side (7 levels of `ofType` nesting covers any
/// realistic `[[Type!]!]!`-style wrapping).
const INTROSPECTION_QUERY: &str = r#"query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
    directives {
      name
      description
      locations
      args { ...InputValue }
    }
  }
}

fragment FullType on __Type {
  kind
  name
  description
  fields(includeDeprecated: true) {
    name
    description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated
    deprecationReason
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) {
    name
    description
    isDeprecated
    deprecationReason
  }
  possibleTypes { ...TypeRef }
}

fragment InputValue on __InputValue {
  name
  description
  type { ...TypeRef }
  defaultValue
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
              ofType {
                kind
                name
              }
            }
          }
        }
      }
    }
  }
}"#;

/// Fetches a GraphQL schema via introspection against the current tab's
/// URL/headers/auth, resolved and sent the same way a real send would be
/// (vars interpolated, auth hydrated + OAuth2-exchanged, workspace transport
/// applied) — but not written to history, and no scripts run. Returns the raw
/// JSON response body (the frontend runs `buildClientSchema` on `data`).
#[tauri::command]
pub async fn introspect_graphql_schema(
    state: State<'_, AppState>,
    mut req: HttpRequest,
    workspace_id: String,
    collection_id: Option<String>,
    request_id: Option<String>,
) -> AppResult<String> {
    req.method = "POST".into();
    req.body = RequestBody::Graphql {
        query: INTROSPECTION_QUERY.into(),
        variables: None,
        operation_name: Some("IntrospectionQuery".into()),
    };

    let resolved = {
        let conn = state.db.lock().unwrap();
        crate::vars::resolve(&conn, &workspace_id, collection_id.as_deref())?
    };
    crate::vars::interpolate_request(&mut req, &resolved.values);

    req.auth = resolve_auth(&state, collection_id.as_deref(), request_id.as_deref()).await?;

    let transport = {
        let conn = state.db.lock().unwrap();
        crate::workspace::apply_default_headers(&mut req, &conn, &workspace_id)?;
        crate::workspace::resolve_transport(&conn, &workspace_id)?
    };

    let resp = crate::engine::http::send(req, Some(Arc::clone(&state.cookie_jar)), transport.as_ref()).await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&resp.body_base64)
        .map_err(|e| AppError::Other(format!("introspection response body is not valid base64: {e}")))?;
    String::from_utf8(bytes)
        .map_err(|e| AppError::Other(format!("introspection response is not valid UTF-8: {e}")))
}
