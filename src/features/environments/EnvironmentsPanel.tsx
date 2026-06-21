//! Environments + scoped variables sidebar panel — workspace-global and
//! per-environment variables, grouped by `groupName`. Creation takes an
//! optional collection scope and group name; editing can rename and change
//! group, but not collection — the store layer only sets `collectionId` at
//! creation, with no reassignment path.

import { useRef, useState, type FocusEvent, type KeyboardEvent, type ReactNode } from "react";
import { Check, ChevronDown, ChevronRight, Circle, Pencil, Plus, Trash2 } from "lucide-react";
import { useActiveWorkspace } from "../workspaces/hooks";
import { useCollections } from "../collections/hooks";
import {
  useActiveEnvironment,
  useCreateEnvironment,
  useDeleteEnvironment,
  useEnvironments,
  useSetActiveEnvironment,
  useUpdateEnvironment,
} from "./hooks";
import { VariablesEditor } from "./VariablesEditor";
import type { Collection, Environment } from "../../lib/types";

export function EnvironmentsPanel() {
  const { data: workspace } = useActiveWorkspace();
  const workspaceId = workspace?.id;
  const { data: environments, isLoading } = useEnvironments(workspaceId);
  const { data: active } = useActiveEnvironment(workspaceId);
  const { data: collections } = useCollections(workspaceId);
  const createEnvironment = useCreateEnvironment(workspaceId);
  const updateEnvironment = useUpdateEnvironment(workspaceId);
  const deleteEnvironment = useDeleteEnvironment(workspaceId);
  const setActiveEnvironment = useSetActiveEnvironment(workspaceId);

  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [globalOpen, setGlobalOpen] = useState(false);
  const [creating, setCreating] = useState(false);

  const groups = new Map<string | null, Environment[]>();
  for (const env of environments ?? []) {
    const arr = groups.get(env.groupName) ?? [];
    arr.push(env);
    groups.set(env.groupName, arr);
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-slate-100 px-2 py-1.5 dark:border-slate-800">
        <span className="text-xs font-semibold tracking-wide text-slate-400 uppercase">Environments</span>
        <button
          type="button"
          onClick={() => setCreating(true)}
          title="New environment"
          className="rounded p-0.5 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
        >
          <Plus size={14} />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        <Section title="Global variables" open={globalOpen} onToggle={() => setGlobalOpen((o) => !o)}>
          <VariablesEditor scope={{ kind: "global" }} />
        </Section>

        {workspaceId && (
          <Section title="Workspace variables" fixed>
            <VariablesEditor scope={{ kind: "workspace", id: workspaceId }} />
          </Section>
        )}

        {creating && (
          <NewEnvironmentRow
            collections={collections ?? []}
            onCommit={(input) => {
              createEnvironment.mutate(input);
              setCreating(false);
            }}
            onCancel={() => setCreating(false)}
          />
        )}

        {isLoading && <p className="px-2 py-1.5 text-xs text-slate-400">Loading…</p>}
        {!isLoading && environments?.length === 0 && !creating && (
          <p className="px-2 py-2 text-xs text-slate-400">No environments yet.</p>
        )}

        {[...groups.entries()].map(([groupName, envs]) => (
          <div key={groupName ?? "__ungrouped"}>
            {groupName && (
              <p className="px-2 pt-2 text-[11px] font-semibold tracking-wide text-slate-400 uppercase">
                {groupName}
              </p>
            )}
            {envs.map((env) => (
              <EnvironmentRow
                key={env.id}
                env={env}
                isActive={env.id === active?.id}
                expanded={env.id === expandedId}
                onToggleExpand={() => setExpandedId(expandedId === env.id ? null : env.id)}
                onSetActive={() => setActiveEnvironment.mutate(env.id === active?.id ? null : env.id)}
                onSave={(name, groupName) => updateEnvironment.mutate({ id: env.id, name, groupName })}
                onDelete={() => {
                  if (window.confirm(`Delete "${env.name}"? This can't be undone.`)) {
                    deleteEnvironment.mutate(env.id);
                  }
                }}
              />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function Section({
  title,
  open,
  onToggle,
  fixed = false,
  children,
}: {
  title: string;
  open?: boolean;
  onToggle?: () => void;
  fixed?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="border-b border-slate-100 pb-1.5 dark:border-slate-800">
      {fixed ? (
        <p className="px-2 pt-1.5 text-[11px] font-semibold tracking-wide text-slate-400 uppercase">{title}</p>
      ) : (
        <button
          type="button"
          onClick={onToggle}
          className="flex w-full items-center gap-1 px-2 pt-1.5 text-[11px] font-semibold tracking-wide text-slate-400 uppercase hover:text-slate-600 dark:hover:text-slate-300"
        >
          {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          {title}
        </button>
      )}
      {(fixed || open) && <div className="mt-1">{children}</div>}
    </div>
  );
}

function EnvironmentRow({
  env,
  isActive,
  expanded,
  onToggleExpand,
  onSetActive,
  onSave,
  onDelete,
}: {
  env: Environment;
  isActive: boolean;
  expanded: boolean;
  onToggleExpand: () => void;
  onSetActive: () => void;
  onSave: (name: string, groupName: string | null) => void;
  onDelete: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(env.name);
  const [groupName, setGroupName] = useState(env.groupName ?? "");
  const committedRef = useRef(false);

  function startEditing() {
    committedRef.current = false;
    setName(env.name);
    setGroupName(env.groupName ?? "");
    setEditing(true);
  }

  function commitEdit() {
    if (committedRef.current) return;
    committedRef.current = true;
    setEditing(false);
    const trimmedName = name.trim() || env.name;
    const trimmedGroup = groupName.trim() || null;
    if (trimmedName !== env.name || trimmedGroup !== env.groupName) onSave(trimmedName, trimmedGroup);
  }

  function cancelEdit() {
    committedRef.current = true;
    setEditing(false);
    setName(env.name);
    setGroupName(env.groupName ?? "");
  }

  function handleEditBlur(e: FocusEvent<HTMLDivElement>) {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) commitEdit();
  }

  function handleEditKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Enter") commitEdit();
    if (e.key === "Escape") cancelEdit();
  }

  return (
    <div>
      <div className="group flex items-center gap-1 px-2 py-1 text-xs hover:bg-slate-100 dark:hover:bg-slate-800">
        <button
          type="button"
          onClick={onToggleExpand}
          className="shrink-0 text-slate-400"
          title={expanded ? "Collapse" : "Expand"}
        >
          {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </button>
        <button
          type="button"
          onClick={onSetActive}
          title={isActive ? "Active environment — click to deactivate" : "Set as active environment"}
          className={"shrink-0 " + (isActive ? "text-accent" : "text-slate-300 dark:text-slate-600")}
        >
          {isActive ? <Check size={13} /> : <Circle size={10} />}
        </button>
        {editing ? (
          <div className="flex min-w-0 flex-1 flex-col gap-0.5" onBlur={handleEditBlur} onKeyDown={handleEditKeyDown}>
            <input
              autoFocus
              value={name}
              onClick={(e) => e.stopPropagation()}
              onChange={(e) => setName(e.target.value)}
              className="min-w-0 rounded border border-accent/40 bg-white px-1 py-0.5 text-xs focus:outline-none dark:bg-slate-900"
            />
            <input
              value={groupName}
              placeholder="Group (optional)"
              onClick={(e) => e.stopPropagation()}
              onChange={(e) => setGroupName(e.target.value)}
              className="min-w-0 rounded border border-slate-200 bg-white px-1 py-0.5 text-xs focus:border-accent/40 focus:outline-none dark:border-slate-700 dark:bg-slate-900"
            />
          </div>
        ) : (
          <span
            onClick={onToggleExpand}
            className={
              "min-w-0 flex-1 truncate cursor-pointer " +
              (isActive ? "font-medium text-slate-700 dark:text-slate-200" : "text-slate-600 dark:text-slate-300")
            }
          >
            {env.name}
          </span>
        )}
        <div className="flex shrink-0 gap-0.5 opacity-0 group-hover:opacity-100">
          <button
            type="button"
            onClick={startEditing}
            title="Rename"
            className="rounded p-0.5 text-slate-400 hover:bg-slate-200 hover:text-slate-700 dark:hover:bg-slate-700"
          >
            <Pencil size={12} />
          </button>
          <button
            type="button"
            onClick={onDelete}
            title="Delete environment"
            className="rounded p-0.5 text-slate-400 hover:bg-red-100 hover:text-red-600 dark:hover:bg-red-900/40"
          >
            <Trash2 size={12} />
          </button>
        </div>
      </div>
      {expanded && (
        <div className="pl-4">
          <VariablesEditor scope={{ kind: "environment", id: env.id }} />
        </div>
      )}
    </div>
  );
}

function NewEnvironmentRow({
  collections,
  onCommit,
  onCancel,
}: {
  collections: Collection[];
  onCommit: (input: { collectionId: string | null; name: string; groupName?: string | null }) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [groupName, setGroupName] = useState("");
  const [collectionId, setCollectionId] = useState("");
  const committedRef = useRef(false);

  function commitOrCancel() {
    if (committedRef.current) return;
    committedRef.current = true;
    const trimmed = name.trim();
    if (!trimmed) {
      onCancel();
      return;
    }
    onCommit({ collectionId: collectionId || null, name: trimmed, groupName: groupName.trim() || null });
  }

  function handleBlur(e: FocusEvent<HTMLDivElement>) {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) commitOrCancel();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Enter") commitOrCancel();
    if (e.key === "Escape") {
      committedRef.current = true;
      onCancel();
    }
  }

  return (
    <div className="space-y-1 px-2 py-1.5" onBlur={handleBlur} onKeyDown={handleKeyDown}>
      <input
        autoFocus
        value={name}
        placeholder="Environment name"
        onChange={(e) => setName(e.target.value)}
        className="w-full rounded border border-accent/40 bg-white px-1.5 py-1 text-xs focus:outline-none dark:bg-slate-900"
      />
      <div className="flex gap-1">
        <input
          value={groupName}
          placeholder="Group (optional)"
          onChange={(e) => setGroupName(e.target.value)}
          className="min-w-0 flex-1 rounded border border-slate-200 bg-white px-1.5 py-1 text-xs focus:border-accent/40 focus:outline-none dark:border-slate-700 dark:bg-slate-900"
        />
        <select
          value={collectionId}
          onChange={(e) => setCollectionId(e.target.value)}
          className="shrink-0 rounded border border-slate-200 bg-white px-1 py-1 text-xs focus:border-accent/40 focus:outline-none dark:border-slate-700 dark:bg-slate-900"
        >
          <option value="">Workspace (global)</option>
          {collections.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}
