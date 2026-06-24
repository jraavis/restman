//! Per-type form fields for an `AuthConfig` — the type picker itself is owned
//! by the caller (`RequestAuthTab` mixes in "Inherit"; `CollectionAuthDialog`
//! doesn't), so this only renders whichever fields `value.type` calls for.
//! Secret fields (`SecretInput`) round-trip `SECRET_MASK` untouched unless
//! the user actually types a new value — see `lib/types.ts`'s `SECRET_MASK`
//! doc comment and `auth::mod`'s mask-on-write contract. The Eye/EyeOff
//! reveal toggle only un-hides local input text, same as `VariablesEditor`;
//! it can't recover a real secret already masked by the backend.

import { useState, type ReactNode } from "react";
import { Eye, EyeOff, Loader2 } from "lucide-react";
import type {
  ApiKeyLocation,
  AuthConfig,
  AuthType,
  AwsSigV4Config,
  OAuth2Config,
  OAuth2GrantType,
  PkceMethod,
} from "../../lib/types";
import {
  useOAuth2Status,
  useOAuthTokenPreview,
  useStartOAuth2Authorization,
} from "./oauthHooks";

export const AUTH_TYPE_LABELS: Record<AuthType, string> = {
  none: "No Auth",
  bearer: "Bearer Token",
  basic: "Basic Auth",
  api_key: "API Key",
  o_auth2: "OAuth 2.0",
  aws_sig_v4: "AWS Signature V4",
};

export const ALL_AUTH_TYPES = Object.keys(AUTH_TYPE_LABELS) as AuthType[];

export const inputClass =
  "w-full rounded-md border border-slate-200 bg-transparent px-2 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-accent/40 dark:border-slate-700";

type AuthScope = { collectionId?: string | null; requestId?: string | null };

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1 text-sm">
      <span className="text-xs text-slate-500 dark:text-slate-400">{label}</span>
      {children}
    </label>
  );
}

export function SecretInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [revealed, setRevealed] = useState(false);
  return (
    <div className="flex items-center gap-1">
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        type={revealed ? "text" : "password"}
        spellCheck={false}
        className={inputClass + " flex-1"}
      />
      <button
        type="button"
        onClick={() => setRevealed((r) => !r)}
        title={revealed ? "Hide value" : "Reveal value"}
        className="shrink-0 rounded p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800"
      >
        {revealed ? <EyeOff size={14} /> : <Eye size={14} />}
      </button>
    </div>
  );
}

export function AuthConfigFields({
  value,
  onChange,
  scope = {},
}: {
  value: AuthConfig;
  onChange: (value: AuthConfig) => void;
  scope?: AuthScope;
}) {
  if (value.type === "none") {
    return <p className="text-sm text-slate-400">No authentication applied.</p>;
  }

  if (value.type === "bearer") {
    return (
      <Field label="Token">
        <SecretInput value={value.token} onChange={(token) => onChange({ ...value, token })} />
      </Field>
    );
  }

  if (value.type === "basic") {
    return (
      <div className="flex flex-col gap-3">
        <Field label="Username">
          <input
            className={inputClass}
            value={value.username}
            onChange={(e) => onChange({ ...value, username: e.target.value })}
          />
        </Field>
        <Field label="Password">
          <SecretInput value={value.password} onChange={(password) => onChange({ ...value, password })} />
        </Field>
      </div>
    );
  }

  if (value.type === "api_key") {
    return (
      <div className="flex flex-col gap-3">
        <Field label="Key">
          <input
            className={inputClass}
            value={value.key}
            onChange={(e) => onChange({ ...value, key: e.target.value })}
            placeholder="X-API-Key"
          />
        </Field>
        <Field label="Value">
          <SecretInput value={value.value} onChange={(v) => onChange({ ...value, value: v })} />
        </Field>
        <Field label="Add to">
          <select
            className={inputClass}
            value={value.location}
            onChange={(e) => onChange({ ...value, location: e.target.value as ApiKeyLocation })}
          >
            <option value="header">Header</option>
            <option value="query">Query Params</option>
          </select>
        </Field>
      </div>
    );
  }

  if (value.type === "o_auth2") {
    return <OAuth2Fields value={value} onChange={onChange} scope={scope} />;
  }

  return <AwsSigV4Fields value={value} onChange={onChange} />;
}

function OAuth2Fields({
  value,
  onChange,
  scope,
}: {
  value: { type: "o_auth2" } & OAuth2Config;
  onChange: (value: AuthConfig) => void;
  scope: AuthScope;
}) {
  const set = <K extends keyof OAuth2Config>(key: K, v: OAuth2Config[K]) => onChange({ ...value, [key]: v });
  const grant = value.grantType;

  return (
    <div className="flex flex-col gap-3">
      <Field label="Grant Type">
        <select className={inputClass} value={grant} onChange={(e) => set("grantType", e.target.value as OAuth2GrantType)}>
          <option value="authorization_code">Authorization Code</option>
          <option value="client_credentials">Client Credentials</option>
          <option value="password">Password Credentials</option>
          <option value="refresh_token">Refresh Token</option>
        </select>
      </Field>

      {grant === "authorization_code" && (
        <Field label="Authorization URL">
          <input
            className={inputClass}
            value={value.authUrl}
            onChange={(e) => set("authUrl", e.target.value)}
            placeholder="https://idp.example.com/authorize"
          />
        </Field>
      )}

      <Field label="Token URL">
        <input
          className={inputClass}
          value={value.tokenUrl}
          onChange={(e) => set("tokenUrl", e.target.value)}
          placeholder="https://idp.example.com/token"
        />
      </Field>

      <Field label="Client ID">
        <input className={inputClass} value={value.clientId} onChange={(e) => set("clientId", e.target.value)} />
      </Field>

      <Field label="Client Secret">
        <SecretInput value={value.clientSecret} onChange={(v) => set("clientSecret", v)} />
      </Field>

      <Field label="Scope">
        <input
          className={inputClass}
          value={value.scope}
          onChange={(e) => set("scope", e.target.value)}
          placeholder="space-separated"
        />
      </Field>

      {grant === "authorization_code" && (
        <>
          <Field label="Redirect URI">
            <input
              className={inputClass}
              value={value.redirectUri}
              onChange={(e) => set("redirectUri", e.target.value)}
              placeholder="http://127.0.0.1:43251/callback"
            />
          </Field>
          <Field label="PKCE">
            <select className={inputClass} value={value.pkce} onChange={(e) => set("pkce", e.target.value as PkceMethod)}>
              <option value="s256">S256</option>
              <option value="plain">Plain</option>
              <option value="none">None</option>
            </select>
          </Field>
        </>
      )}

      {grant === "password" && (
        <>
          <Field label="Username">
            <input className={inputClass} value={value.username} onChange={(e) => set("username", e.target.value)} />
          </Field>
          <Field label="Password">
            <SecretInput value={value.password} onChange={(v) => set("password", v)} />
          </Field>
        </>
      )}

      {grant === "refresh_token" && (
        <Field label="Refresh Token">
          <SecretInput value={value.refreshToken} onChange={(v) => set("refreshToken", v)} />
        </Field>
      )}

      <div className="flex flex-col gap-2 border-t border-slate-100 pt-3 dark:border-slate-800">
        {grant === "authorization_code" && <OAuth2Connect scope={scope} />}
        <OAuth2StatusDisplay scope={scope} />
      </div>
    </div>
  );
}

function OAuth2Connect({ scope }: { scope: AuthScope }) {
  const start = useStartOAuth2Authorization(scope);
  const scoped = !!(scope.collectionId || scope.requestId);

  return (
    <div className="flex flex-col gap-1">
      <button
        type="button"
        disabled={!scoped || start.isPending}
        onClick={() => start.mutate()}
        title={scoped ? undefined : "Save first to connect"}
        className="flex w-fit items-center gap-1.5 rounded-md border border-slate-200 px-3 py-1.5 text-sm font-medium text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
      >
        {start.isPending && <Loader2 size={14} className="animate-spin" />}
        {start.isPending ? "Waiting for sign-in…" : "Connect"}
      </button>
      {!scoped && <p className="text-xs text-slate-400">Save the request first to connect.</p>}
      {start.isError && (
        <p className="text-xs text-red-500">
          {start.error instanceof Error ? start.error.message : String(start.error)}
        </p>
      )}
    </div>
  );
}

function OAuth2StatusDisplay({ scope }: { scope: AuthScope }) {
  const { data: status } = useOAuth2Status(scope);
  const { data: preview } = useOAuthTokenPreview(scope);
  if (!status?.connected) {
    return <p className="text-xs text-slate-400">Not connected.</p>;
  }
  return (
    <p className="text-xs text-emerald-600 dark:text-emerald-400">
      Connected
      {status.expiresAt
        ? ` — expires ${new Date(status.expiresAt).toLocaleString()}`
        : ""}
      {status.scope ? ` · ${status.scope}` : ""}
      {preview ? (
        <span
          className="ml-2 rounded bg-slate-100 px-1.5 py-0.5 font-mono text-slate-500 dark:bg-slate-800 dark:text-slate-400"
          title="Masked token preview — raw value never leaves the backend"
        >
          {preview}
        </span>
      ) : null}
    </p>
  );
}

function AwsSigV4Fields({
  value,
  onChange,
}: {
  value: { type: "aws_sig_v4" } & AwsSigV4Config;
  onChange: (value: AuthConfig) => void;
}) {
  const set = <K extends keyof AwsSigV4Config>(key: K, v: AwsSigV4Config[K]) => onChange({ ...value, [key]: v });
  return (
    <div className="flex flex-col gap-3">
      <Field label="Access Key ID">
        <input className={inputClass} value={value.accessKey} onChange={(e) => set("accessKey", e.target.value)} />
      </Field>
      <Field label="Secret Access Key">
        <SecretInput value={value.secretKey} onChange={(v) => set("secretKey", v)} />
      </Field>
      <Field label="Region">
        <input
          className={inputClass}
          value={value.region}
          onChange={(e) => set("region", e.target.value)}
          placeholder="us-east-1"
        />
      </Field>
      <Field label="Service">
        <input
          className={inputClass}
          value={value.service}
          onChange={(e) => set("service", e.target.value)}
          placeholder="execute-api"
        />
      </Field>
      <Field label="Session Token (optional)">
        <SecretInput value={value.sessionToken} onChange={(v) => set("sessionToken", v)} />
      </Field>
    </div>
  );
}
