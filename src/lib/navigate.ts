/** Cross-feature navigation without prop drilling: App listens for this. */
export type NavTarget = "mail" | "chat" | "settings";

// Views that mount only while shown (Settings) miss the event that opens
// them, so the latest detail per target waits here until they take it.
const pending = new Map<NavTarget, Record<string, unknown>>();

export function takePendingNav(target: NavTarget) {
  const d = pending.get(target);
  pending.delete(target);
  return d;
}

export function navigateTo(target: NavTarget, detail?: Record<string, unknown>) {
  if (detail) pending.set(target, detail);
  else pending.delete(target);
  window.dispatchEvent(new CustomEvent("tc:navigate", { detail: { target, ...detail } }));
}

export function onNavigate(cb: (target: NavTarget, detail: Record<string, unknown>) => void) {
  const handler = (e: Event) => {
    const d = (e as CustomEvent).detail as { target: NavTarget } & Record<string, unknown>;
    cb(d.target, d);
  };
  window.addEventListener("tc:navigate", handler);
  return () => window.removeEventListener("tc:navigate", handler);
}
