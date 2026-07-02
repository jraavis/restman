//! JWT decode and optional signature verification for the developer tools panel.

import { base64ToBytes, base64UrlToBytes, prettyJson, type ToolResult } from "./encoding";

export interface JwtClaimInfo {
  name: string;
  raw: unknown;
  formatted: string;
  expired?: boolean;
}

export interface DecodedJwt {
  header: Record<string, unknown>;
  payload: Record<string, unknown>;
  signature: string;
  headerJson: string;
  payloadJson: string;
  claims: JwtClaimInfo[];
  warnings: string[];
}

export interface JwtVerifyResult {
  verified: boolean;
  algorithm: string;
  error?: string;
}

const TIME_CLAIMS = ["exp", "iat", "nbf"] as const;

function parseJsonPart(bytes: Uint8Array, part: string): ToolResult<Record<string, unknown>> {
  try {
    const text = new TextDecoder().decode(bytes);
    const value = JSON.parse(text);
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, error: `${part} is not a JSON object` };
    }
    return { ok: true, value: value as Record<string, unknown> };
  } catch {
    return { ok: false, error: `Invalid ${part} JSON` };
  }
}

function formatClaim(name: string, raw: unknown, now = Date.now()): JwtClaimInfo {
  if (typeof raw === "number" && TIME_CLAIMS.includes(name as (typeof TIME_CLAIMS)[number])) {
    const ms = raw < 1e12 ? raw * 1000 : raw;
    const date = new Date(ms);
    const formatted = Number.isNaN(date.getTime()) ? String(raw) : date.toISOString();
    const expired = name === "exp" && ms < now;
    return { name, raw, formatted, expired };
  }
  return { name, raw, formatted: String(raw) };
}

export function decodeJwt(token: string, now = Date.now()): ToolResult<DecodedJwt> {
  const trimmed = token.trim().replace(/^Bearer\s+/i, "");
  if (!trimmed) return { ok: false, error: "Token is empty" };

  const parts = trimmed.split(".");
  if (parts.length !== 3) return { ok: false, error: "JWT must have exactly 3 dot-separated parts" };

  const warnings: string[] = [];
  let header: Record<string, unknown>;
  let payload: Record<string, unknown>;

  try {
    const headerResult = parseJsonPart(base64UrlToBytes(parts[0]), "header");
    if (!headerResult.ok) return headerResult;
    header = headerResult.value;

    const payloadResult = parseJsonPart(base64UrlToBytes(parts[1]), "payload");
    if (!payloadResult.ok) return payloadResult;
    payload = payloadResult.value;
  } catch {
    return { ok: false, error: "Invalid base64url in token" };
  }

  const alg = typeof header.alg === "string" ? header.alg : "unknown";
  if (alg.toLowerCase() === "none") warnings.push('Algorithm is "none" — token is unsigned');

  const headerJson = prettyJson(JSON.stringify(header)) ?? JSON.stringify(header);
  const payloadJson = prettyJson(JSON.stringify(payload)) ?? JSON.stringify(payload);

  const claims = TIME_CLAIMS.filter((c) => c in payload).map((c) =>
    formatClaim(c, payload[c], now),
  );

  return {
    ok: true,
    value: {
      header,
      payload,
      signature: parts[2],
      headerJson,
      payloadJson,
      claims,
      warnings,
    },
  };
}

function pemToBinary(pem: string): Uint8Array<ArrayBuffer> {
  const b64 = pem
    .replace(/-----BEGIN [^-]+-----/g, "")
    .replace(/-----END [^-]+-----/g, "")
    .replace(/\s+/g, "");
  return Uint8Array.from(base64ToBytes(b64));
}

function copyBytes(bytes: Uint8Array): Uint8Array<ArrayBuffer> {
  return Uint8Array.from(bytes);
}

const HMAC_ALGOS: Record<string, string> = {
  HS256: "SHA-256",
  HS384: "SHA-384",
  HS512: "SHA-512",
};

const RSA_ALGOS: Record<string, string> = {
  RS256: "RSASSA-PKCS1-v1_5",
  RS384: "RSASSA-PKCS1-v1_5",
  RS512: "RSASSA-PKCS1-v1_5",
};

const RSA_HASH: Record<string, string> = {
  RS256: "SHA-256",
  RS384: "SHA-384",
  RS512: "SHA-512",
};

export async function verifyJwt(
  token: string,
  secretOrKey: string,
): Promise<ToolResult<JwtVerifyResult>> {
  const decoded = decodeJwt(token);
  if (!decoded.ok) return decoded;

  const trimmed = token.trim().replace(/^Bearer\s+/i, "");
  const parts = trimmed.split(".");
  const alg = typeof decoded.value.header.alg === "string" ? decoded.value.header.alg : "";

  if (!secretOrKey.trim()) {
    return { ok: false, error: "Secret or public key is required for verification" };
  }
  if (alg.toLowerCase() === "none") {
    return { ok: true, value: { verified: false, algorithm: alg, error: 'Cannot verify unsigned "none" algorithm' } };
  }

  const signingInput = new TextEncoder().encode(`${parts[0]}.${parts[1]}`);
  const signature = base64UrlToBytes(parts[2]);

  try {
    if (alg in HMAC_ALGOS) {
      const key = await crypto.subtle.importKey(
        "raw",
        new TextEncoder().encode(secretOrKey),
        { name: "HMAC", hash: HMAC_ALGOS[alg] },
        false,
        ["verify"],
      );
      const verified = await crypto.subtle.verify("HMAC", key, copyBytes(signature), signingInput);
      return { ok: true, value: { verified, algorithm: alg, error: verified ? undefined : "Signature mismatch" } };
    }

    if (alg in RSA_ALGOS) {
      const key = await crypto.subtle.importKey(
        "spki",
        pemToBinary(secretOrKey),
        { name: RSA_ALGOS[alg], hash: RSA_HASH[alg] },
        false,
        ["verify"],
      );
      const verified = await crypto.subtle.verify(RSA_ALGOS[alg], key, copyBytes(signature), signingInput);
      return { ok: true, value: { verified, algorithm: alg, error: verified ? undefined : "Signature mismatch" } };
    }

    return { ok: true, value: { verified: false, algorithm: alg, error: `Unsupported algorithm: ${alg}` } };
  } catch (e) {
    const message = e instanceof Error ? e.message : "Verification failed";
    return { ok: true, value: { verified: false, algorithm: alg, error: message } };
  }
}

export function formatDecodedJwt(decoded: DecodedJwt): string {
  const lines = [
    "── Header ──",
    decoded.headerJson,
    "",
    "── Payload ──",
    decoded.payloadJson,
    "",
    `── Signature ──`,
    decoded.signature,
  ];

  if (decoded.claims.length > 0) {
    lines.push("", "── Time claims ──");
    for (const c of decoded.claims) {
      const flag = c.expired ? " (EXPIRED)" : "";
      lines.push(`${c.name}: ${c.formatted}${flag}`);
    }
  }

  if (decoded.warnings.length > 0) {
    lines.push("", "── Warnings ──", ...decoded.warnings);
  }

  return lines.join("\n");
}