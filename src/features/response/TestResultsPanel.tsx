//! Displays the pass/fail breakdown of pre- and post-request scripts after a send.

import { CheckCircle, XCircle, AlertCircle } from "lucide-react";
import type { ScriptResult } from "../../lib/types";

interface TestResultsPanelProps {
  preScript: ScriptResult | null;
  postScript: ScriptResult | null;
}

export function TestResultsPanel({
  preScript,
  postScript,
}: TestResultsPanelProps) {
  const sections = [
    { label: "Pre-request", result: preScript },
    { label: "Post-response", result: postScript },
  ].filter((s) => s.result !== null && (s.result.tests.length > 0 || s.result.error));

  if (sections.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-slate-400">
        No tests ran — add scripts in the Scripts tab.
      </div>
    );
  }

  const allTests = sections.flatMap((s) => s.result!.tests);
  const totalPassed = allTests.filter((t) => t.passed).length;
  const totalFailed = allTests.filter((t) => !t.passed).length;

  return (
    <div className="flex flex-col gap-4 overflow-auto p-3 text-sm">
      {/* Summary bar */}
      <div className="flex items-center gap-3 rounded-lg bg-slate-50 px-3 py-2 dark:bg-slate-800">
        <span className="flex items-center gap-1.5 font-medium text-emerald-600 dark:text-emerald-400">
          <CheckCircle size={14} />
          {totalPassed} passed
        </span>
        {totalFailed > 0 && (
          <span className="flex items-center gap-1.5 font-medium text-red-500 dark:text-red-400">
            <XCircle size={14} />
            {totalFailed} failed
          </span>
        )}
        <span className="ml-auto text-xs text-slate-500">
          {allTests.length} total
        </span>
      </div>

      {/* Per-section breakdown */}
      {sections.map(({ label, result }) => (
        <div key={label}>
          <div className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
            {label}
          </div>

          {/* Runtime error (not a test failure) */}
          {result!.error && (
            <div className="mb-2 flex items-start gap-2 rounded-lg border border-red-200 bg-red-50 p-2 text-xs text-red-700 dark:border-red-800 dark:bg-red-900/20 dark:text-red-400">
              <AlertCircle size={13} className="mt-0.5 shrink-0" />
              <span className="font-mono break-all">{result!.error}</span>
            </div>
          )}

          {/* Individual test rows */}
          {result!.tests.length > 0 ? (
            <div className="flex flex-col divide-y divide-slate-100 rounded-lg border border-slate-200 dark:divide-slate-700 dark:border-slate-700">
              {result!.tests.map((t, i) => (
                <div
                  key={i}
                  className="flex items-start gap-2.5 px-3 py-2"
                >
                  {t.passed ? (
                    <CheckCircle
                      size={13}
                      className="mt-0.5 shrink-0 text-emerald-500"
                    />
                  ) : (
                    <XCircle
                      size={13}
                      className="mt-0.5 shrink-0 text-red-500"
                    />
                  )}
                  <div className="min-w-0 flex-1">
                    <div
                      className={
                        "truncate font-medium " +
                        (t.passed
                          ? "text-slate-700 dark:text-slate-200"
                          : "text-red-700 dark:text-red-400")
                      }
                    >
                      {t.name}
                    </div>
                    {!t.passed && t.error && (
                      <div className="mt-0.5 font-mono text-xs text-slate-500 dark:text-slate-400 break-all">
                        {t.error}
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          ) : (
            !result!.error && (
              <div className="text-xs text-slate-400">No tests defined.</div>
            )
          )}
        </div>
      ))}
    </div>
  );
}
