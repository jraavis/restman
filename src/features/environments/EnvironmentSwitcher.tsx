//! Active-environment indicator + quick-switch popover, mounted in the
//! TopBar. Cmd/Ctrl+E opens it, mirroring TabsBar's Cmd+1..9 shortcut.

import { useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import { useRegisterCommand } from "../../lib/commands";
import { useDismissable } from "../../lib/useDismissable";
import { useActiveEnvironment, useEnvironments, useSetActiveEnvironment } from "./hooks";
import type { Environment } from "../../lib/types";

export function EnvironmentSwitcher({ workspaceId }: { workspaceId: string | undefined }) {
  const { data: environments } = useEnvironments(workspaceId);
  const { data: active } = useActiveEnvironment(workspaceId);
  const setActive = useSetActiveEnvironment(workspaceId);
  const [open, setOpen] = useState(false);
  const ref = useDismissable<HTMLDivElement>(() => setOpen(false));

  useRegisterCommand("environment.switch", () => setOpen((o) => !o));

  const groups = new Map<string | null, Environment[]>();
  for (const env of environments ?? []) {
    const arr = groups.get(env.groupName) ?? [];
    arr.push(env);
    groups.set(env.groupName, arr);
  }

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        title="Active environment (⌘E)"
        className="flex items-center gap-1 rounded-full bg-slate-100 px-2.5 py-1 text-xs text-slate-500 hover:bg-slate-200 dark:bg-slate-800 dark:text-slate-400 dark:hover:bg-slate-700"
      >
        {active ? active.name : "No environment"}
        <ChevronDown size={12} />
      </button>
      {open && (
        <div className="absolute right-0 top-full z-50 mt-1 w-52 rounded-md border border-slate-200 bg-white py-1 text-xs shadow-lg dark:border-slate-700 dark:bg-slate-800">
          <button
            type="button"
            onClick={() => {
              setActive.mutate(null);
              setOpen(false);
            }}
            className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
          >
            <span className="w-3.5 shrink-0">{!active && <Check size={12} />}</span>
            No environment
          </button>
          {(environments?.length ?? 0) > 0 && (
            <div className="my-1 border-t border-slate-100 dark:border-slate-700" />
          )}
          {[...groups.entries()].map(([groupName, envs]) => (
            <div key={groupName ?? "__ungrouped"}>
              {groupName && (
                <p className="px-3 pt-1 text-[10px] font-semibold tracking-wide text-slate-400 uppercase">
                  {groupName}
                </p>
              )}
              {envs.map((env) => (
                <button
                  key={env.id}
                  type="button"
                  onClick={() => {
                    setActive.mutate(env.id);
                    setOpen(false);
                  }}
                  className="flex w-full items-center gap-1.5 px-3 py-1.5 text-left hover:bg-slate-100 dark:hover:bg-slate-700"
                >
                  <span className="w-3.5 shrink-0">{active?.id === env.id && <Check size={12} />}</span>
                  <span className="truncate">{env.name}</span>
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
