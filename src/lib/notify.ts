import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
// Inlined as data URLs: the webview media stack wants HTTP range requests
// that the app's asset server does not provide, so <audio src> stays
// silent. Decoding the bytes ourselves and playing through Web Audio
// works the same on WebView2 and WebKitGTK.
import mailWav from "../assets/sounds/mail.wav?inline";
import chatWav from "../assets/sounds/chat.wav?inline";

let granted: boolean | null = null;
let ctx: AudioContext | null = null;
const buffers: Partial<Record<"mail" | "chat", AudioBuffer>> = {};
const sources = { mail: mailWav, chat: chatWav };

export interface NotifyPrefs {
  notifications: boolean;
  sound: boolean;
}

export const notifyPrefs: NotifyPrefs = { notifications: true, sound: true };

function bytesOf(dataUrl: string): ArrayBuffer {
  const b64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out.buffer;
}

async function buffer(name: "mail" | "chat"): Promise<AudioBuffer> {
  ctx ??= new AudioContext();
  return (buffers[name] ??= await ctx.decodeAudioData(bytesOf(sources[name])));
}

// Browsers keep the context suspended until the first user gesture; wake it
// then so a notification arriving later can make noise.
if (typeof document !== "undefined") {
  const unlock = () => {
    ctx ??= new AudioContext();
    void ctx.resume();
    void buffer("mail");
    void buffer("chat");
  };
  document.addEventListener("pointerdown", unlock, { once: true });
  document.addEventListener("keydown", unlock, { once: true });
}

export async function playSound(name: "mail" | "chat") {
  if (!notifyPrefs.sound) return;
  try {
    const buf = await buffer(name);
    if (ctx!.state === "suspended") await ctx!.resume();
    const src = ctx!.createBufferSource();
    src.buffer = buf;
    src.connect(ctx!.destination);
    src.start();
  } catch (e) {
    console.warn("sound failed", e);
  }
}

export async function notify(title: string, body: string, sound: "mail" | "chat") {
  void playSound(sound);
  if (!notifyPrefs.notifications) return;
  if (granted === null) {
    granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
  }
  if (granted) sendNotification({ title, body });
}
