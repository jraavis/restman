//! HTTP method metadata: the standard set, color coding, and protocol/status
//! helpers used across the request builder and response viewer.

export const HTTP_METHODS = [
  "GET",
  "POST",
  "PUT",
  "PATCH",
  "DELETE",
  "HEAD",
  "OPTIONS",
  "TRACE",
  "CONNECT",
] as const;

/** Scheme of a URL (http, https, ws, wss, grpc, …) or null if absent. */
export function protocolOf(url: string): string | null {
  const m = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(url.trim());
  return m ? m[1].toLowerCase() : null;
}

export function isValidUrl(url: string): boolean {
  try {
    new URL(url.trim());
    return true;
  } catch {
    return false;
  }
}

/** Tailwind bg+border+text classes for a colored method pill (selects, badges). */
export function methodBadgeClasses(method: string): string {
  switch (method.toUpperCase()) {
    case "GET":
      return "bg-green-500/10 border-green-500/30 text-green-700 dark:text-green-400";
    case "POST":
      return "bg-blue-500/10 border-blue-500/30 text-blue-700 dark:text-blue-400";
    case "PUT":
      return "bg-amber-500/10 border-amber-500/30 text-amber-700 dark:text-amber-400";
    case "PATCH":
      return "bg-purple-500/10 border-purple-500/30 text-purple-700 dark:text-purple-400";
    case "DELETE":
      return "bg-red-500/10 border-red-500/30 text-red-700 dark:text-red-400";
    case "HEAD":
    case "OPTIONS":
      return "bg-teal-500/10 border-teal-500/30 text-teal-700 dark:text-teal-400";
    default:
      return "bg-slate-500/10 border-slate-500/30 text-slate-700 dark:text-slate-400";
  }
}

/** Tailwind classes for a status-code badge (2xx green … 5xx red). */
export function statusColor(status: number): string {
  if (status >= 200 && status < 300)
    return "bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400";
  if (status >= 300 && status < 400)
    return "bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-400";
  if (status >= 400 && status < 500)
    return "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-400";
  if (status >= 500)
    return "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-400";
  return "bg-slate-100 text-slate-700 dark:bg-slate-800 dark:text-slate-300";
}

/** Which lucide icon (by name) best represents a status code's outcome. */
export function statusIconName(status: number): "check-circle-2" | "alert-triangle" | "x-circle" | "arrow-right-circle" | "help-circle" {
  if (status >= 200 && status < 300) return "check-circle-2";
  if (status >= 300 && status < 400) return "arrow-right-circle";
  if (status >= 400 && status < 500) return "alert-triangle";
  if (status >= 500) return "x-circle";
  return "help-circle";
}
