import { app } from "./ipc";
// Inlined as data URLs: the webview media stack wants HTTP range requests
// that the app's asset server does not provide, so <audio src> stays
// silent. Decoding the bytes ourselves and playing through Web Audio
// works the same on WebView2 and WebKitGTK.
import chime from "../assets/sounds/chime.wav?inline";
import pop from "../assets/sounds/pop.wav?inline";
import ding from "../assets/sounds/ding.wav?inline";
import knock from "../assets/sounds/knock.wav?inline";
import bubble from "../assets/sounds/bubble.wav?inline";
import marimba from "../assets/sounds/marimba.wav?inline";
import triple from "../assets/sounds/triple.wav?inline";

export const SOUNDS = {
  chime: { label: "Chime", data: chime },
  ding: { label: "Ding", data: ding },
  pop: { label: "Pop", data: pop },
  bubble: { label: "Bubble", data: bubble },
  knock: { label: "Knock", data: knock },
  marimba: { label: "Marimba", data: marimba },
  triple: { label: "Triple", data: triple },
} as const;

export type SoundName = keyof typeof SOUNDS;
export const SOUND_NAMES = Object.keys(SOUNDS) as SoundName[];

export interface NotifyPrefs {
  notifications: boolean;
  sound: boolean;
  mail: SoundName;
  chat: SoundName;
}

export const notifyPrefs: NotifyPrefs = { notifications: true, sound: true, mail: "chime", chat: "pop" };

export function asSoundName(s: string | undefined, fallback: SoundName): SoundName {
  return s && s in SOUNDS ? (s as SoundName) : fallback;
}

let ctx: AudioContext | null = null;
const buffers: Partial<Record<SoundName, AudioBuffer>> = {};

function bytesOf(dataUrl: string): ArrayBuffer {
  const b64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out.buffer;
}

async function buffer(name: SoundName): Promise<AudioBuffer> {
  ctx ??= new AudioContext();
  return (buffers[name] ??= await ctx.decodeAudioData(bytesOf(SOUNDS[name].data)));
}

// Browsers keep the context suspended until the first user gesture; wake it
// then so a notification arriving later can make noise.
if (typeof document !== "undefined") {
  const unlock = () => {
    ctx ??= new AudioContext();
    void ctx.resume();
    void buffer(notifyPrefs.mail);
    void buffer(notifyPrefs.chat);
  };
  document.addEventListener("pointerdown", unlock, { once: true });
  document.addEventListener("keydown", unlock, { once: true });
}

/** Plays a specific sound regardless of preferences (for previews). */
export async function playNamed(name: SoundName) {
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

export function playSound(kind: "mail" | "chat") {
  if (!notifyPrefs.sound) return;
  void playNamed(notifyPrefs[kind]);
}

/**
 * Sound plus a native notification. `route` (a conecta:// link) is opened
 * when the notification is clicked, restoring the window from the tray.
 */
export async function notify(title: string, body: string, sound: "mail" | "chat", route?: string) {
  playSound(sound);
  if (!notifyPrefs.notifications) return;
  await app.notify(title, body, route).catch((e) => console.warn("notify failed", e));
}
