//! Read-only browser for a schema fetched via `useGraphqlSchema`. Root types
//! (Query/Mutation/Subscription) first since those are what a user is
//! actually about to write a query against, then every other named type
//! alphabetically. Introspection meta-types (`__Schema` etc.) and built-in
//! scalars are omitted — noise no one browsing docs wants.

import { useState } from "react";
import { ChevronRight } from "lucide-react";
import {
  isEnumType,
  isInputObjectType,
  isInterfaceType,
  isObjectType,
  isScalarType,
  isUnionType,
  type GraphQLField,
  type GraphQLInputField,
  type GraphQLNamedType,
  type GraphQLSchema,
} from "graphql";

const BUILTIN_SCALARS = new Set(["String", "Int", "Float", "Boolean", "ID"]);

interface Props {
  schema: GraphQLSchema;
  /** Called with a field or type name when the user clicks it — the caller
   * decides what "insert" means (e.g. splice into the query editor). */
  onInsert?: (name: string) => void;
}

export function GraphqlDocsExplorer({ schema, onInsert }: Props) {
  const rootCandidates: { label: string; type: GraphQLNamedType | null | undefined }[] = [
    { label: "Query", type: schema.getQueryType() },
    { label: "Mutation", type: schema.getMutationType() },
    { label: "Subscription", type: schema.getSubscriptionType() },
  ];
  const rootTypes: { label: string; type: GraphQLNamedType }[] = [];
  for (const r of rootCandidates) {
    if (r.type != null) rootTypes.push({ label: r.label, type: r.type });
  }

  const rootNames = new Set(rootTypes.map((r) => r.type.name));
  const otherTypes = Object.values(schema.getTypeMap())
    .filter((t) => !t.name.startsWith("__") && !BUILTIN_SCALARS.has(t.name) && !rootNames.has(t.name))
    .sort((a, b) => a.name.localeCompare(b.name));

  return (
    <div className="flex flex-col gap-1 overflow-y-auto text-sm">
      {rootTypes.map((r) => (
        <TypeSection key={r.label} label={r.label} type={r.type} onInsert={onInsert} defaultOpen />
      ))}
      {otherTypes.length > 0 && (
        <div className="mt-2 border-t border-slate-200 pt-2 dark:border-slate-800">
          <p className="px-1 pb-1 text-xs font-semibold tracking-wide text-slate-400 uppercase dark:text-slate-500">
            Types
          </p>
          {otherTypes.map((t) => (
            <TypeSection key={t.name} label={t.name} type={t} onInsert={onInsert} />
          ))}
        </div>
      )}
    </div>
  );
}

function TypeSection({
  label,
  type,
  onInsert,
  defaultOpen,
}: {
  label: string;
  type: GraphQLNamedType;
  onInsert?: (name: string) => void;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(!!defaultOpen);
  const entries = childEntries(type);

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-1 rounded-md px-1 py-1 text-left font-medium text-slate-700 hover:bg-slate-100 dark:text-slate-200 dark:hover:bg-slate-800"
      >
        <ChevronRight size={12} className={"shrink-0 transition-transform " + (open ? "rotate-90" : "")} />
        {label}
        <span className="text-xs font-normal text-slate-400">{kindLabel(type)}</span>
      </button>
      {open && (
        <div className="ml-4 flex flex-col gap-0.5 border-l border-slate-200 pl-2 dark:border-slate-800">
          {entries.length === 0 && <p className="py-1 text-xs text-slate-400">No fields.</p>}
          {entries.map((e) => (
            <button
              key={e.name}
              type="button"
              onClick={() => onInsert?.(e.name)}
              className="flex items-baseline gap-2 rounded px-1 py-0.5 text-left hover:bg-accent/10"
              title={e.description ?? undefined}
            >
              <span className="text-slate-800 dark:text-slate-100">{e.name}</span>
              <span className="truncate text-xs text-slate-400">{e.typeString}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function kindLabel(t: GraphQLNamedType): string {
  if (isObjectType(t)) return "type";
  if (isInterfaceType(t)) return "interface";
  if (isUnionType(t)) return "union";
  if (isEnumType(t)) return "enum";
  if (isInputObjectType(t)) return "input";
  if (isScalarType(t)) return "scalar";
  return "";
}

function childEntries(t: GraphQLNamedType): { name: string; typeString: string; description?: string | null }[] {
  if (isObjectType(t) || isInterfaceType(t)) {
    return Object.values(t.getFields()).map((f: GraphQLField<unknown, unknown>) => ({
      name: f.name,
      typeString: f.type.toString(),
      description: f.description,
    }));
  }
  if (isInputObjectType(t)) {
    return Object.values(t.getFields()).map((f: GraphQLInputField) => ({
      name: f.name,
      typeString: f.type.toString(),
      description: f.description,
    }));
  }
  if (isEnumType(t)) {
    return t.getValues().map((v) => ({ name: v.name, typeString: "", description: v.description }));
  }
  if (isUnionType(t)) {
    return t.getTypes().map((m) => ({ name: m.name, typeString: "", description: null }));
  }
  return [];
}
