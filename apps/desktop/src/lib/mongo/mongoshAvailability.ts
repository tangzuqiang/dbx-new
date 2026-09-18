import * as api from "@/lib/backend/api";
import type { MongoshStatus } from "@/lib/backend/tauri";

let statusRequest: Promise<MongoshStatus> | undefined;

/** Detect the executable through the desktop process PATH, once per app session. */
export function getMongoshStatus(): Promise<MongoshStatus> {
  statusRequest ??= api.mongoShellStatus().catch(() => ({ installed: false, version: null }));
  return statusRequest;
}
