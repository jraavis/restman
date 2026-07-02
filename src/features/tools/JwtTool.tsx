import { useEffect, useMemo, useState } from "react";
import { decodeJwt, formatDecodedJwt, verifyJwt } from "../../lib/jwt";
import { ToolLayout } from "./ToolLayout";

export function JwtTool() {
  const [token, setToken] = useState("");
  const [secret, setSecret] = useState("");
  const [verifyResult, setVerifyResult] = useState<string | undefined>();

  const { output, error } = useMemo(() => {
    if (!token.trim()) return { output: "", error: undefined };
    const result = decodeJwt(token);
    if (!result.ok) return { output: "", error: result.error };
    return { output: formatDecodedJwt(result.value), error: undefined };
  }, [token]);

  useEffect(() => {
    if (!secret.trim() || !token.trim()) {
      setVerifyResult(undefined);
      return;
    }
    let cancelled = false;
    void verifyJwt(token, secret).then((result) => {
      if (cancelled) return;
      if (!result.ok) {
        setVerifyResult(result.error);
        return;
      }
      const { verified, algorithm, error: verifyError } = result.value;
      if (verified) {
        setVerifyResult(`Signature valid (${algorithm})`);
      } else {
        setVerifyResult(verifyError ?? `Signature invalid (${algorithm})`);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [token, secret]);

  return (
    <ToolLayout
      input={token}
      onInputChange={setToken}
      inputLabel="JWT token"
      output={output}
      error={error}
      extra={
        <label className="flex min-w-0 flex-col gap-1">
          <span className="text-xs font-medium text-slate-500 dark:text-slate-400">
            Secret or public key (optional, for verification)
          </span>
          <input
            type="password"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
            placeholder="HMAC secret or PEM public key"
            className="w-full min-w-0 rounded-md border border-slate-200 bg-slate-50 px-3 py-1.5 font-mono text-sm text-slate-800 focus:border-accent/50 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100"
          />
          {verifyResult && (
            <span
              className={
                "text-xs " +
                (verifyResult.startsWith("Signature valid")
                  ? "text-green-600 dark:text-green-400"
                  : "text-amber-600 dark:text-amber-400")
              }
            >
              {verifyResult}
            </span>
          )}
        </label>
      }
    />
  );
}