//! GraphQL schema introspection state. Manual "Fetch schema" trigger, not
//! auto-fetch-on-keystroke — introspection is a real network call through
//! `introspect_graphql_schema`, so firing it on every URL edit would be
//! wasteful and noisy. State is local to whatever calls the hook (not
//! persisted, not shared across tabs) since a fetched schema is ephemeral
//! diagnostic data, same posture as the SSE/WS/gRPC panels' connection state.
//!
//! The `graphql` package is dynamically imported (not a top-level import)
//! since this hook is used from `RequestBuilder`, which sits outside this
//! app's lazy-Monaco boundary — a static import would put `graphql` in the
//! main bundle for every user, not just ones who open the GraphQL body mode.

import { useCallback, useState } from "react";
import type { GraphQLSchema } from "graphql";
import type { HttpRequest } from "../../lib/http";
import { ipc } from "../../lib/ipc";

export type GraphqlSchemaStatus = "idle" | "loading" | "ready" | "error";

export interface GraphqlSchemaState {
  status: GraphqlSchemaStatus;
  schema: GraphQLSchema | null;
  error: string | null;
  fetchSchema: (req: HttpRequest, workspaceId: string, collectionId: string | null, requestId: string | null) => void;
}

/** Parses a raw `{ data: { __schema: ... } }` introspection response body into
 * a `GraphQLSchema`. Throws (doesn't swallow) on invalid JSON, a GraphQL
 * error response (`{ errors: [...] }`), or a malformed introspection shape —
 * callers surface the message as the fetch's `error` state. */
export async function schemaFromIntrospectionResponse(rawBody: string): Promise<GraphQLSchema> {
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
  const { buildClientSchema } = await import("graphql");
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
        .then((raw) => schemaFromIntrospectionResponse(raw))
        .then((parsedSchema) => {
          setSchema(parsedSchema);
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
