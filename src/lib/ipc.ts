//! Typed wrappers around Tauri IPC commands. The frontend never touches the
//! network or disk directly — every backend operation goes through here.

import { invoke } from "@tauri-apps/api/core";
import type { HttpRequest, HttpResponse } from "./http";

export interface Workspace {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  isActive: boolean;
}

export const ipc = {
  ping: () => invoke<string>("ping"),
  sendRequest: (req: HttpRequest) => invoke<HttpResponse>("send_request", { req }),
  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),
  activeWorkspace: () => invoke<Workspace | null>("active_workspace"),
  createWorkspace: (name: string) => invoke<Workspace>("create_workspace", { name }),
  setActiveWorkspace: (id: string) => invoke<void>("set_active_workspace", { id }),
};
