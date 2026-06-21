//! Center request builder, with the open-tabs bar above it.

import { RequestBuilder } from "../features/request/RequestBuilder";
import { TabsBar } from "../features/tabs/TabsBar";

export function RequestPane() {
  return (
    <div className="flex h-full min-w-0 flex-col">
      <TabsBar />
      <div className="min-h-0 flex-1 overflow-auto">
        <RequestBuilder />
      </div>
    </div>
  );
}
