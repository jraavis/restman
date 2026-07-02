import { useMemo, useState } from "react";
import { minifyJson, prettyJson } from "../../lib/encoding";
import { ToolLayout } from "./ToolLayout";

export function JsonTool() {
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<"format" | "minify">("format");

  const { output, error } = useMemo(() => {
    if (!input.trim()) return { output: "", error: undefined };
    const result = mode === "format" ? prettyJson(input) : minifyJson(input);
    if (result === null) return { output: "", error: "Invalid JSON" };
    return { output: result, error: undefined };
  }, [input, mode]);

  return (
    <ToolLayout
      input={input}
      onInputChange={setInput}
      output={output}
      error={error}
      mode={mode}
      modes={[
        { id: "format", label: "Format" },
        { id: "minify", label: "Minify" },
      ]}
      onModeChange={(id) => setMode(id as "format" | "minify")}
    />
  );
}