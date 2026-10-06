import { create } from "zustand";
import type { Update } from "@tauri-apps/plugin-updater";
import { errorMessage } from "./ipc";

/**
 * One updater shared by the background schedule, the "ready" banner and
 * Settings → About. Releases come from GitHub (`latest.json`, see
 * tauri.conf.json); background checks stay quiet on errors, manual ones
 * report them.
 */
export type UpdatePhase = "idle" | "checking" | "upToDate" | "available" | "downloading" | "ready" | "installing" | "error";

interface UpdaterState {
  phase: UpdatePhase;
  version: string | null;
  notes: string | null;
  downloaded: number;
  total: number | null;
  error: string | null;
  /** The banner was closed for this version. */
  dismissed: boolean;
  check: (manual: boolean) => Promise<void>;
  download: (manual: boolean) => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

const AUTO_KEY = "tc.autoDownloadUpdates";
export const getAutoDownload = () => localStorage.getItem(AUTO_KEY) !== "false";
export const setAutoDownload = (v: boolean) => localStorage.setItem(AUTO_KEY, String(v));

// Not in the store: it's a backend resource handle, not render state.
let pending: Update | null = null;

export const useUpdater = create<UpdaterState>((set, get) => ({
  phase: "idle",
  version: null,
  notes: null,
  downloaded: 0,
  total: null,
  error: null,
  dismissed: false,

  check: async (manual) => {
    const { phase } = get();
    if (phase === "checking" || phase === "downloading" || phase === "installing") return;
    if (phase === "ready") {
      if (manual) set({ dismissed: false });
      return;
    }
    set({ phase: "checking", error: null });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check({ timeout: 30_000 });
      if (!update) {
        set({ phase: manual ? "upToDate" : "idle" });
        return;
      }
      const isNew = update.version !== get().version;
      if (pending && pending !== update) void pending.close().catch(() => {});
      pending = update;
      set({
        phase: "available",
        version: update.version,
        notes: update.body?.trim() || null,
        dismissed: isNew ? false : get().dismissed,
      });
      if (manual || getAutoDownload()) await get().download(manual);
    } catch (e) {
      fail(set, manual, "update check failed", e);
    }
  },

  download: async (manual) => {
    const update = pending;
    if (!update || get().phase !== "available") return;
    set({ phase: "downloading", downloaded: 0, total: null, error: null });
    try {
      await update.download((ev) => {
        if (ev.event === "Started") set({ total: ev.data.contentLength ?? null });
        else if (ev.event === "Progress") set((s) => ({ downloaded: s.downloaded + ev.data.chunkLength }));
      });
      set({ phase: "ready", dismissed: false });
    } catch (e) {
      // Back to "available" so the download can be retried.
      fail(set, manual, "update download failed", e, "available");
    }
  },

  install: async () => {
    const update = pending;
    if (!update || get().phase !== "ready") return;
    set({ phase: "installing", error: null });
    try {
      // On Windows this hands over to the installer, which exits the app and
      // starts the new version; elsewhere we relaunch ourselves.
      await update.install();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      set({ phase: "ready", error: errorMessage(e), dismissed: false });
    }
  },

  dismiss: () => set({ dismissed: true }),
}));

function fail(
  set: (p: Partial<UpdaterState>) => void,
  manual: boolean,
  what: string,
  e: unknown,
  quietPhase: UpdatePhase = "idle",
) {
  console.warn(`${what}:`, errorMessage(e));
  set(manual ? { phase: quietPhase === "available" ? "available" : "error", error: errorMessage(e) } : { phase: quietPhase });
}

const FIRST_CHECK_MS = 30_000;
const EVERY_MS = 6 * 60 * 60 * 1000;

/** Checks shortly after launch and then every few hours. Off in dev builds. */
export function startUpdateChecks() {
  if (import.meta.env.DEV) return () => {};
  const run = () => void useUpdater.getState().check(false);
  const first = window.setTimeout(run, FIRST_CHECK_MS);
  const every = window.setInterval(run, EVERY_MS);
  return () => {
    window.clearTimeout(first);
    window.clearInterval(every);
  };
}
