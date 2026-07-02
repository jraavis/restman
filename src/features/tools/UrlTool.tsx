import { useMemo, useState } from "react";
import { decodeUrl, encodeUrl } from "../../lib/url";
import { ToolLayout } from "./ToolLayout";

export function UrlTool() {
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<"decode" | "encode">("decode");

  const { output, error } = useMemo(() => {
    if (!input.trim()) return { output: "", error: undefined };
    const result = mode === "decode" ? decodeUrl(input) : encodeUrl(input);
    if (!result.ok) return { output: "", error: result.error };
    return { output: result.value, error: undefined };
  }, [input, mode]);

  return (
    <ToolLayout
      input={input}
      onInputChange={setInput}
      output={output}
      error={error}
      mode={mode}
      modes={[
        { id: "decode", label: "Decode" },
        { id: "encode", label: "Encode" },
      ]}
      onModeChange={(id) => setMode(id as "decode" | "encode")}
    />
  );
}