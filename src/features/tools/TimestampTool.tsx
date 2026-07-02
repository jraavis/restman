import { useMemo, useState } from "react";
import { formatTimestampInfo, nowTimestamp, parseTimestamp } from "../../lib/timestamp";
import { ToolLayout } from "./ToolLayout";

export function TimestampTool() {
  const [input, setInput] = useState("");

  const { output, error } = useMemo(() => {
    if (!input.trim()) return { output: "", error: undefined };
    const result = parseTimestamp(input);
    if (!result.ok) return { output: "", error: result.error };
    return { output: formatTimestampInfo(result.value), error: undefined };
  }, [input]);

  return (
    <ToolLayout
      input={input}
      onInputChange={setInput}
      inputLabel="Timestamp (Unix seconds/ms or ISO-8601)"
      output={output}
      error={error}
      actions={
        <button
          type="button"
          onClick={() => setInput(String(nowTimestamp().milliseconds))}
          className="rounded-md border border-slate-200 px-2.5 py-1 text-xs text-slate-600 hover:bg-slate-100 dark:border-slate-600 dark:text-slate-300 dark:hover:bg-slate-700"
        >
          Now
        </button>
      }
    />
  );
}