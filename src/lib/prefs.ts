/** Small UI preferences that only matter on this machine (localStorage). */

export type StartIn = "last" | "chat" | "mail";
export type RailItem = "mail" | "chat";

const read = <T>(key: string, fallback: T): T => {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
};
const write = (key: string, v: unknown) => localStorage.setItem(key, JSON.stringify(v));

export const getStartIn = () => read<StartIn>("tc.startIn", "last");
export const setStartIn = (v: StartIn) => write("tc.startIn", v);

export const getLastTab = () => read<RailItem>("tc.lastTab", "mail");
export const setLastTab = (v: RailItem) => write("tc.lastTab", v);

export const getRailOrder = (): RailItem[] => {
  const v = read<RailItem[]>("tc.railOrder", ["mail", "chat"]);
  return v.includes("mail") && v.includes("chat") ? v : ["mail", "chat"];
};
export const setRailOrder = (v: RailItem[]) => write("tc.railOrder", v);

/** Look up sender logos (BIMI, Gravatar, site favicon) — opt in, since it pings third parties. */
export const getSenderLogos = () => read<boolean>("tc.senderLogos", false);
export const setSenderLogos = (v: boolean) => write("tc.senderLogos", v);
