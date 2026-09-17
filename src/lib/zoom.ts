import { getCurrentWebview } from "@tauri-apps/api/webview";

const KEY = "tc.zoom";
const STEPS = [0.7, 0.8, 0.9, 1, 1.1, 1.25, 1.4, 1.6, 1.8, 2];
let current = 1;
const listeners = new Set<(z: number) => void>();

export function zoomLevel() {
  return current;
}

export function onZoom(cb: (z: number) => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export async function setZoom(z: number) {
  current = Math.min(2, Math.max(0.7, Math.round(z * 100) / 100));
  localStorage.setItem(KEY, String(current));
  await getCurrentWebview().setZoom(current);
  listeners.forEach((cb) => cb(current));
}

export function zoomStep(dir: 1 | -1) {
  const i = STEPS.findIndex((s) => Math.abs(s - current) < 0.01);
  const next = i === -1 ? 1 : STEPS[Math.min(STEPS.length - 1, Math.max(0, i + dir))];
  return setZoom(next);
}

export async function restoreZoom() {
  const saved = Number(localStorage.getItem(KEY));
  if (saved && saved !== 1) await setZoom(saved);
}
