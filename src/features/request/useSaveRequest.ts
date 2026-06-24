//! Saves the live draft to its collection: creates a new `SavedRequest` the
//! first time (then links the active tab to it so later saves overwrite
//! rather than re-create), or updates the existing one in place once linked.

import { useCreateRequest, useUpdateRequest } from "../collections/hooks";
import { useSetTabRequestId } from "../tabs/hooks";
import { useRequestStore } from "../../stores/requestStore";
import type { SavedRequestInput } from "../../lib/types";

export function useSaveRequest(workspaceId: string | undefined) {
  const requestId = useRequestStore((s) => s.requestId);
  const activeTabId = useRequestStore((s) => s.activeTabId);
  const title = useRequestStore((s) => s.title);
  const request = useRequestStore((s) => s.request);
  const auth = useRequestStore((s) => s.auth);
  const preRequestScript = useRequestStore((s) => s.preRequestScript);
  const postResponseScript = useRequestStore((s) => s.postResponseScript);
  const setRequestLink = useRequestStore((s) => s.setRequestLink);

  const createRequest = useCreateRequest(workspaceId);
  const updateRequest = useUpdateRequest();
  const setTabRequestId = useSetTabRequestId(workspaceId);

  const isLinked = requestId !== null;

  /**
   * Updates in place if already linked; otherwise requires `collectionId` for
   * the first save. `name` overrides the current title for the saved
   * request's name field — takes a direct arg rather than reading the store
   * after a `setTitle` call, since `title` here is captured at render time
   * and a same-tick `setTitle` then `save()` would otherwise still see the
   * stale value.
   */
  async function save(collectionId?: string, name?: string) {
    const input: SavedRequestInput = {
      name: name ?? title,
      method: request.method,
      url: request.url,
      headers: request.headers,
      query: request.query,
      body: request.body,
      options: request.options,
      auth,
      preRequestScript,
      postResponseScript,
    };

    if (requestId) {
      return updateRequest.mutateAsync({ id: requestId, input });
    }

    if (!collectionId) {
      throw new Error("save: collectionId is required for the first save");
    }
    const saved = await createRequest.mutateAsync({ collectionId, input });
    setRequestLink(saved.id, saved.collectionId);
    if (activeTabId) {
      await setTabRequestId.mutateAsync({ id: activeTabId, requestId: saved.id });
    }
    return saved;
  }

  return {
    save,
    isLinked,
    saving: createRequest.isPending || updateRequest.isPending,
  };
}
