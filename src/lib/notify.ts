import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

let granted: boolean | null = null;
const sounds: Record<string, HTMLAudioElement> = {};

export interface NotifyPrefs {
  notifications: boolean;
  sound: boolean;
}

export const notifyPrefs: NotifyPrefs = { notifications: true, sound: true };

export function playSound(name: "mail" | "chat") {
  if (!notifyPrefs.sound) return;
  const el = (sounds[name] ??= new Audio(`/sounds/${name}.wav`));
  el.currentTime = 0;
  void el.play().catch(() => {
    /* autoplay policies never block here (user gesture not required in webview), but be safe */
  });
}

export async function notify(title: string, body: string, sound: "mail" | "chat") {
  playSound(sound);
  if (!notifyPrefs.notifications) return;
  if (granted === null) {
    granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
  }
  if (granted) sendNotification({ title, body });
}
