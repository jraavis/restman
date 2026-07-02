//! Timestamp conversion helpers for the developer tools panel.

import type { ToolResult } from "./encoding";

export interface TimestampInfo {
  inputKind: "seconds" | "milliseconds" | "iso";
  seconds: number;
  milliseconds: number;
  isoUtc: string;
  local: string;
  relative: string;
}

function formatRelative(ms: number, now: number): string {
  const diff = now - ms;
  const abs = Math.abs(diff);
  const future = diff < 0;
  const suffix = future ? "from now" : "ago";

  const units: [number, string][] = [
    [365 * 24 * 60 * 60 * 1000, "year"],
    [30 * 24 * 60 * 60 * 1000, "month"],
    [24 * 60 * 60 * 1000, "day"],
    [60 * 60 * 1000, "hour"],
    [60 * 1000, "minute"],
    [1000, "second"],
  ];

  for (const [unitMs, label] of units) {
    const n = Math.floor(abs / unitMs);
    if (n >= 1) {
      const plural = n === 1 ? "" : "s";
      return `${n} ${label}${plural} ${suffix}`;
    }
  }
  return "just now";
}

export function parseTimestamp(input: string, now = Date.now()): ToolResult<TimestampInfo> {
  const trimmed = input.trim();
  if (!trimmed) return { ok: false, error: "Input is empty" };

  if (/^\d+$/.test(trimmed)) {
    const n = Number(trimmed);
    const ms = trimmed.length >= 13 ? n : n * 1000;
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) return { ok: false, error: "Invalid timestamp" };
    return {
      ok: true,
      value: {
        inputKind: trimmed.length >= 13 ? "milliseconds" : "seconds",
        seconds: Math.floor(ms / 1000),
        milliseconds: ms,
        isoUtc: date.toISOString(),
        local: date.toLocaleString(),
        relative: formatRelative(ms, now),
      },
    };
  }

  const date = new Date(trimmed);
  if (Number.isNaN(date.getTime())) return { ok: false, error: "Invalid date string" };
  const ms = date.getTime();
  return {
    ok: true,
    value: {
      inputKind: "iso",
      seconds: Math.floor(ms / 1000),
      milliseconds: ms,
      isoUtc: date.toISOString(),
      local: date.toLocaleString(),
      relative: formatRelative(ms, now),
    },
  };
}

export function formatTimestampInfo(info: TimestampInfo): string {
  const lines = [
    `Input type: ${info.inputKind}`,
    `Unix seconds: ${info.seconds}`,
    `Unix milliseconds: ${info.milliseconds}`,
    `ISO (UTC): ${info.isoUtc}`,
    `Local: ${info.local}`,
    `Relative: ${info.relative}`,
  ];
  return lines.join("\n");
}

export function nowTimestamp(now = Date.now()): TimestampInfo {
  const date = new Date(now);
  return {
    inputKind: "milliseconds",
    seconds: Math.floor(now / 1000),
    milliseconds: now,
    isoUtc: date.toISOString(),
    local: date.toLocaleString(),
    relative: "just now",
  };
}