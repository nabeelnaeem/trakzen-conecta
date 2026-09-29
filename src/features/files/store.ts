import { create } from "zustand";
import { errorMessage, files } from "../../lib/ipc";
import type { FilesStatus, FilesUploadSource } from "../../lib/types";

/** What the Save dialog is saving; `name` is shown and used as the default file name. */
export interface SaveRequest {
  source: FilesUploadSource;
  name: string;
}

interface FilesState {
  status: FilesStatus | null;
  /** The file currently being saved through the dialog, if any. */
  saving: (SaveRequest & { id: number }) | null;
  refresh: (check?: boolean) => Promise<void>;
  setStatus: (s: FilesStatus) => void;
  save: (req: SaveRequest) => void;
  close: () => void;
}

export const useFiles = create<FilesState>((set) => ({
  status: null,
  saving: null,
  refresh: async (check = false) => {
    try {
      set({ status: await files.status(check) });
    } catch (e) {
      console.warn("Trakzen Files status:", errorMessage(e));
    }
  },
  setStatus: (status) => set({ status }),
  save: (req) => set({ saving: { ...req, id: Date.now() } }),
  close: () => set({ saving: null }),
}));

/** Opens the "Save to Trakzen Files" dialog for a chat file or mail attachment. */
export const saveToFiles = (req: SaveRequest) => useFiles.getState().save(req);

const LAST_DIR = "conecta.files.lastDir";
export const lastDir = () => localStorage.getItem(LAST_DIR) ?? "/shared";
export const rememberDir = (dir: string) => localStorage.setItem(LAST_DIR, dir);

/** `/shared/Wedding/Day 1` → `Shared › Wedding › Day 1`. */
export function displayPath(path: string): string {
  const parts = path.split("/").filter(Boolean);
  if (!parts.length) return "Trakzen Files";
  const space = parts[0] === "home" ? "My files" : parts[0] === "shared" ? "Shared" : parts[0];
  return [space, ...parts.slice(1)].join(" › ");
}
