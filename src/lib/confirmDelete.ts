//! Shared gate for delete confirmations. The Settings → General
//! `confirmBeforeDelete` toggle suppresses the prompt for single-entity
//! deletes (workspace/collection/request/environment/tag/plugin/mock
//! server). Bulk destructive actions (clear history, restore-from-backup)
//! deliberately do NOT consult it — those always prompt.

import { useUiStore } from "../stores/uiStore";

/** True when the delete may proceed: either confirmation is disabled in
 * settings, or the user accepted the prompt. */
export function confirmDelete(message: string): boolean {
  return !useUiStore.getState().confirmBeforeDelete || window.confirm(message);
}
