import type { NavTarget } from "./navigate";

/**
 * The tab App is showing. Every view stays mounted, so window-level keyboard
 * handlers check this to stay quiet while their tab is hidden.
 */
let current: NavTarget = "mail";
const listeners = new Set<(t: NavTarget) => void>();

export const activeTab = (): NavTarget => current;

export function setActiveTab(t: NavTarget) {
  if (t === current) return;
  current = t;
  for (const l of listeners) l(t);
}

export function onActiveTab(cb: (t: NavTarget) => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}
