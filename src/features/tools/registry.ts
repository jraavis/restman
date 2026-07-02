import type { ComponentType } from "react";
import { Base64Tool } from "./Base64Tool";
import { HashTool } from "./HashTool";
import { HexTool } from "./HexTool";
import { JsonTool } from "./JsonTool";
import { JwtTool } from "./JwtTool";
import { TimestampTool } from "./TimestampTool";
import { UrlTool } from "./UrlTool";

export type ToolId = "base64" | "jwt" | "url" | "hex" | "json" | "timestamp" | "hash";

export const TOOLS: { id: ToolId; label: string; component: ComponentType }[] = [
  { id: "base64", label: "Base64", component: Base64Tool },
  { id: "jwt", label: "JWT", component: JwtTool },
  { id: "url", label: "URL", component: UrlTool },
  { id: "hex", label: "Hex", component: HexTool },
  { id: "json", label: "JSON", component: JsonTool },
  { id: "timestamp", label: "Timestamp", component: TimestampTool },
  { id: "hash", label: "Hash", component: HashTool },
];