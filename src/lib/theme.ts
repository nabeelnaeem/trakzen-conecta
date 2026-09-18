/**
 * Appearance: mode (light / dark / high contrast / follow system), accent,
 * density and font, plus optional custom token overrides. Everything is
 * expressed as CSS variables that Tailwind's utilities already read
 * (`--color-gray-50`, `--color-blue-600`, `--spacing`, `--font-sans`), so
 * the whole UI re-themes without touching components. Stored in
 * localStorage and applied before first paint to avoid a flash.
 */

export type Mode = "light" | "dark" | "contrast" | "system";
export type Accent = "blue" | "indigo" | "violet" | "teal" | "emerald" | "rose" | "orange";
export type Density = "comfortable" | "compact";
export type FontChoice = "system" | "inter" | "serif" | "mono";

export interface CustomTheme {
  name: string;
  /** Which built-in palette the overrides sit on top of. */
  base: Exclude<Mode, "system">;
  /** CSS variable → value, e.g. { "--color-gray-50": "#f4f0ea" }. */
  tokens: Record<string, string>;
}

export interface ThemePrefs {
  mode: Mode;
  accent: Accent;
  density: Density;
  font: FontChoice;
  /** Render HTML mail on white paper (default) or invert it in dark mode. */
  mailDark: "paper" | "invert";
  custom: CustomTheme | null;
}

export const ACCENTS: Accent[] = ["blue", "indigo", "violet", "teal", "emerald", "rose", "orange"];

/** The tokens a custom theme may set, in the order the editor shows them. */
export const THEME_TOKENS: { key: string; label: string }[] = [
  { key: "--color-gray-50", label: "Background (sidebar)" },
  { key: "--color-white", label: "Surface (panels, cards)" },
  { key: "--color-gray-100", label: "Surface hover" },
  { key: "--color-gray-200", label: "Border" },
  { key: "--color-gray-500", label: "Muted text" },
  { key: "--color-gray-900", label: "Text" },
  { key: "--color-blue-600", label: "Accent" },
  { key: "--color-blue-700", label: "Accent hover" },
  { key: "--bubble-own", label: "Chat bubble (mine)" },
  { key: "--bubble-peer", label: "Chat bubble (peer)" },
  { key: "--radius", label: "Corner radius" },
];

const KEY = "tc.theme";
const listeners = new Set<(p: ThemePrefs) => void>();

export const defaultPrefs: ThemePrefs = {
  mode: "system",
  accent: "blue",
  density: "comfortable",
  font: "system",
  mailDark: "paper",
  custom: null,
};

let current: ThemePrefs = load();

function load(): ThemePrefs {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? { ...defaultPrefs, ...(JSON.parse(raw) as Partial<ThemePrefs>) } : { ...defaultPrefs };
  } catch {
    return { ...defaultPrefs };
  }
}

export function themePrefs(): ThemePrefs {
  return current;
}

export function onTheme(cb: (p: ThemePrefs) => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

const systemDark = () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;

/** The palette actually in effect once "system" is resolved. */
export function effectiveMode(p: ThemePrefs = current): Exclude<Mode, "system"> {
  if (p.custom) return p.custom.base;
  if (p.mode === "system") return systemDark() ? "dark" : "light";
  return p.mode;
}

export function isDark(p: ThemePrefs = current): boolean {
  return effectiveMode(p) === "dark";
}

export function applyTheme(p: ThemePrefs = current) {
  const root = document.documentElement;
  root.dataset.theme = effectiveMode(p);
  root.dataset.accent = p.accent;
  root.dataset.density = p.density;
  root.dataset.font = p.font;
  root.style.colorScheme = isDark(p) ? "dark" : "light";
  // Custom tokens go inline so they win over the palette stylesheet.
  for (const t of THEME_TOKENS) root.style.removeProperty(t.key);
  if (p.custom) {
    for (const [k, v] of Object.entries(p.custom.tokens)) {
      if (k.startsWith("--")) root.style.setProperty(k, v);
    }
  }
}

export function setTheme(patch: Partial<ThemePrefs>) {
  current = { ...current, ...patch };
  localStorage.setItem(KEY, JSON.stringify(current));
  applyTheme(current);
  listeners.forEach((cb) => cb(current));
}

export function toggleDark() {
  setTheme({ mode: isDark() ? "light" : "dark", custom: null });
}

export function exportThemeJson(p: ThemePrefs = current): string {
  const theme: CustomTheme = p.custom ?? { name: "My theme", base: effectiveMode(p), tokens: {} };
  return JSON.stringify({ ...theme, accent: p.accent, density: p.density, font: p.font }, null, 2);
}

export function importThemeJson(json: string): void {
  const parsed = JSON.parse(json) as Partial<CustomTheme> & Partial<ThemePrefs>;
  if (!parsed || typeof parsed !== "object") throw new Error("Not a theme file");
  const base = (["light", "dark", "contrast"] as const).includes(parsed.base as never) ? parsed.base! : "light";
  const tokens: Record<string, string> = {};
  for (const [k, v] of Object.entries(parsed.tokens ?? {})) {
    if (k.startsWith("--") && typeof v === "string" && v.length < 64) tokens[k] = v;
  }
  setTheme({
    custom: { name: typeof parsed.name === "string" ? parsed.name : "Imported theme", base, tokens },
    ...(parsed.accent && ACCENTS.includes(parsed.accent) ? { accent: parsed.accent } : {}),
    ...(parsed.density === "compact" || parsed.density === "comfortable" ? { density: parsed.density } : {}),
    ...(parsed.font ? { font: parsed.font } : {}),
  });
}

/** Call once at startup; re-applies when the OS theme flips under "system". */
export function initTheme() {
  applyTheme(current);
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (current.mode === "system" && !current.custom) {
      applyTheme(current);
      listeners.forEach((cb) => cb(current));
    }
  });
}
