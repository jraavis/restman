import { useEffect, useState } from "react";
import type { HashAlgorithm } from "../../lib/hash";
import { hashText } from "../../lib/hash";
import { ToolLayout } from "./ToolLayout";

const ALGORITHMS: { id: HashAlgorithm; label: string }[] = [
  { id: "md5", label: "MD5" },
  { id: "sha256", label: "SHA-256" },
  { id: "sha384", label: "SHA-384" },
  { id: "sha512", label: "SHA-512" },
];

export function HashTool() {
  const [input, setInput] = useState("");
  const [algorithm, setAlgorithm] = useState<HashAlgorithm>("sha256");
  const [output, setOutput] = useState("");
  const [error, setError] = useState<string | undefined>();

  useEffect(() => {
    if (!input) {
      setOutput("");
      setError(undefined);
      return;
    }
    let cancelled = false;
    void hashText(input, algorithm).then((result) => {
      if (cancelled) return;
      if (!result.ok) {
        setOutput("");
        setError(result.error);
      } else {
        setOutput(result.value);
        setError(undefined);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [input, algorithm]);

  return (
    <ToolLayout
      input={input}
      onInputChange={setInput}
      output={output}
      error={error}
      mode={algorithm}
      modes={ALGORITHMS}
      onModeChange={(id) => setAlgorithm(id as HashAlgorithm)}
    />
  );
}