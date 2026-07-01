//! Lazy-loaded docs explorer. `GraphqlDocsExplorer` imports the `graphql`
//! package for its type-guard functions (isObjectType etc.) — deferred behind
//! React.lazy so it only loads once a schema is fetched and the panel opens,
//! same rationale as `LazyCodeEditor` deferring monaco-editor.

import { lazy, Suspense } from "react";
import type { GraphQLSchema } from "graphql";

const GraphqlDocsExplorer = lazy(() =>
  import("./GraphqlDocsExplorer").then((m) => ({ default: m.GraphqlDocsExplorer })),
);

interface Props {
  schema: GraphQLSchema;
  onInsert?: (name: string) => void;
}

export function LazyGraphqlDocsExplorer(props: Props) {
  return (
    <Suspense fallback={<div className="p-2 text-xs text-slate-400">Loading docs…</div>}>
      <GraphqlDocsExplorer {...props} />
    </Suspense>
  );
}
