// The update prompt (F11, AC11.1): what the About page and the tray panel show, from `update_status`
// and `update:changed`. Rust decides what is on offer (skip, Later); the view only renders it.
import { useEffect, useState } from "react";
import { api, onUpdateChanged } from "./api";
import type { UpdateDto } from "./api-types";

/**
 * What the About page shows: the offer with its three buttons, the install under way, "no new version"
 * after a check the person asked for, or nothing but the Check button.
 */
export type UpdateView = "offer" | "installing" | "current" | "idle";

export function updateView(update: UpdateDto | null, checked: boolean): UpdateView {
  if (update?.installing) return "installing";
  if (update?.available) return "offer";
  return checked ? "current" : "idle";
}

/** The status, loaded once and kept current by `update:changed`. Null until the first answer. */
export function useUpdate(): [UpdateDto | null, (update: UpdateDto) => void] {
  const [update, setUpdate] = useState<UpdateDto | null>(null);
  useEffect(() => {
    let stopped = false;
    let off: (() => void) | undefined;
    // No IPC (plain browser, tests): no prompt.
    onUpdateChanged(setUpdate).then(
      (unlisten) => (stopped ? unlisten() : (off = unlisten)),
      () => undefined,
    );
    api.updateStatus().then(setUpdate, () => undefined);
    return () => {
      stopped = true;
      off?.();
    };
  }, []);
  return [update, setUpdate];
}
