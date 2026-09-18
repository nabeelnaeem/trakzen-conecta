/** Cross-feature navigation without prop drilling: App listens for this. */
export type NavTarget = "mail" | "chat" | "settings";

export function navigateTo(target: NavTarget, detail?: Record<string, unknown>) {
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
