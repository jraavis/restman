//! GraphQL schema introspection state. Manual "Fetch schema" trigger, not
//! auto-fetch-on-keystroke — introspection is a real network call through
//! `introspect_graphql_schema`, so firing it on every URL edit would be
//! wasteful and noisy. State is local to whatever calls the hook (not
//! persisted, not shared across tabs) since a fetched schema is ephemeral
//! diagnostic data, same posture as the SSE/WS/gRPC panels' connection state.

import { useCallback, useState } from "react";
import { buildClientSchema, getIntrospectionQuery, type GraphQLSchema } from "graphql";
import type { HttpRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";

export type GraphqlSchemaStatus = "idle" | "loading" | "ready" | "error";

export interface GraphqlSchemaState {
  status: GraphqlSchemaStatus;
  schema: GraphQLSchema | null;
  error: string | null;
  fetchSchema: (req: HttpRequest, workspaceId: string, collectionId: string | null, requestId: string | null) => void;
}

/** The query text `introspect_graphql_schema` actually sends is a Rust-side
 * constant (kept in sync manually); this is only used to confirm the shape
 * a response should have before parsing, not sent anywhere from the frontend. */
export const INTROSPECTION_QUERY_FOR_REFERENCE = getIntrospectionQuery();

/** Parses a raw `{ data: { __schema: ... } }` introspection response body into
 * a `GraphQLSchema`. Throws (doesn't swallow) on invalid JSON, a GraphQL
 * error response (`{ errors: [...] }`), or a malformed introspection shape —
 * callers surface the message as the fetch's `error` state. */
export function schemaFromIntrospectionResponse(rawBody: string): GraphQLSchema {
  const parsed = JSON.parse(rawBody);
  if (parsed.errors) {
    const messages = Array.isArray(parsed.errors)
      ? parsed.errors.map((e: { message?: string }) => e.message ?? String(e)).join("; ")
      : String(parsed.errors);
    throw new Error(`Server returned GraphQL errors: ${messages}`);
  }
  if (!parsed.data?.__schema) {
    throw new Error("Response has no __schema field — is this a GraphQL endpoint?");
  }
  return buildClientSchema(parsed.data);
}

export function useGraphqlSchema(): GraphqlSchemaState {
  const [status, setStatus] = useState<GraphqlSchemaStatus>("idle");
  const [schema, setSchema] = useState<GraphQLSchema | null>(null);
  const [error, setError] = useState<string | null>(null);

  const fetchSchema = useCallback(
    (req: HttpRequest, workspaceId: string, collectionId: string | null, requestId: string | null) => {
      setStatus("loading");
      setError(null);
      ipc
        .introspectGraphqlSchema(req, workspaceId, collectionId, requestId)
        .then((raw) => {
          setSchema(schemaFromIntrospectionResponse(raw));
          setStatus("ready");
        })
        .catch((e) => {
          setError(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
          setStatus("error");
        });
    },
    [],
  );

  return { status, schema, error, fetchSchema };
}
