import { useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { disable as autostartDisable, enable as autostartEnable, isEnabled as autostartEnabled } from "@tauri-apps/plugin-autostart";
import { errorMessage, mail, settings } from "../../lib/ipc";
import { asSoundName, notifyPrefs, playNamed, SOUND_NAMES, SOUNDS, type SoundName } from "../../lib/notify";
import { onZoom, setZoom, zoomLevel, zoomStep } from "../../lib/zoom";
import { bytes } from "../../lib/format";
import { chat as chatIpc } from "../../lib/ipc";
import {
  ACCENTS,
  exportThemeJson,
  importThemeJson,
  onTheme,
  setTheme,
  themePrefs,
  THEME_TOKENS,
  type Accent,
  type Density,
  type FontChoice,
  type Mode,
} from "../../lib/theme";
import { getSenderLogos, getStartIn, setSenderLogos, setStartIn, type StartIn } from "../../lib/prefs";
import type { Account, MailFilter, NewFilter, SettingsPatch, SettingsView as Settings, StorageStats } from "../../lib/types";
import { useChat } from "../chat/store";
import { useMail } from "../mail/store";
import { Spinner } from "../../lib/Spinner";

type Tab = "general" | "appearance" | "mail" | "accounts" | "filters" | "chat" | "about";

const TABS: { key: Tab; label: string }[] = [
  { key: "general", label: "General" },
  { key: "appearance", label: "Appearance" },
  { key: "mail", label: "Mail" },
  { key: "accounts", label: "Accounts" },
  { key: "filters", label: "Filters" },
  { key: "chat", label: "Chat" },
  { key: "about", label: "About" },
];

/**
 * Settings autosave: every control writes through `save()` which patches
 * the backend immediately and shows a brief "Saved" mark. There is no Save
 * button; the only exceptions are free-text fields, which save on blur.
 */
export function SettingsView() {
  const [tab, setTab] = useState<Tab>("general");
  const [s, setS] = useState<Settings | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const refreshIdentity = useChat((c) => c.refreshIdentity);
  const applyMail = useMail((m) => m.applySettings);

  useEffect(() => {
    settings.get().then(setS).catch((e) => setErr(errorMessage(e)));
  }, []);

  const save = useCallback(
    async (patch: SettingsPatch, note?: string) => {
      setErr(null);
      try {
        const v = await settings.update(patch);
        setS(v);
        applyMail({ showImages: v.mailShowImages, conversations: v.conversationView, undoSeconds: v.undoSendSeconds });
        notifyPrefs.notifications = v.notifications;
        notifyPrefs.sound = v.notificationSound;
        notifyPrefs.mail = asSoundName(v.soundMail, "chime");
        notifyPrefs.chat = asSoundName(v.soundChat, "pop");
        setSavedAt(Date.now());
        if (patch.chatDisplayName !== undefined) void refreshIdentity();
        if (note) setErr(note);
      } catch (e) {
        setErr(errorMessage(e));
      }
    },
    [applyMail, refreshIdentity],
  );

  if (!s) return <div className="p-6 text-sm text-gray-500">{err ?? "Loading…"}</div>;

  return (
    <div className="flex h-full">
      <nav className="w-48 shrink-0 border-r border-gray-200 bg-gray-50 p-2">
        <div className="px-3 pt-2 pb-3 text-base font-semibold">Settings</div>
        {TABS.map((t) => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`block w-full rounded-md px-3 py-1.5 text-left text-sm ${tab === t.key ? "bg-blue-100 font-medium text-blue-900" : "text-gray-700 hover:bg-gray-200"}`}
          >
            {t.label}
          </button>
        ))}
      </nav>
      <div className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-2xl p-6">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="text-lg font-semibold">{TABS.find((t) => t.key === tab)?.label}</h2>
            <SavedMark at={savedAt} />
          </div>
          {err && (
            <div className="mb-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800">
              {err}
            </div>
          )}
          {tab === "general" && <GeneralTab s={s} save={save} />}
          {tab === "appearance" && <AppearanceTab />}
          {tab === "mail" && <MailTab s={s} save={save} />}
          {tab === "accounts" && <AccountsTab s={s} save={save} />}
          {tab === "filters" && <FiltersTab />}
          {tab === "chat" && <ChatTab s={s} save={save} />}
          {tab === "about" && <AboutTab />}
        </div>
      </div>
    </div>
  );
}

function SavedMark({ at }: { at: number | null }) {
  const [show, setShow] = useState(false);
  useEffect(() => {
    if (!at) return;
    setShow(true);
    const t = window.setTimeout(() => setShow(false), 1500);
    return () => window.clearTimeout(t);
  }, [at]);
  return <span className={`text-xs text-green-700 transition-opacity ${show ? "opacity-100" : "opacity-0"}`}>Saved</span>;
}

type Save = (patch: SettingsPatch, note?: string) => Promise<void>;

function Toggle({ v, on, label, hint }: { v: boolean; on: (v: boolean) => void; label: string; hint?: string }) {
  return (
    <div>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={v} onChange={(e) => on(e.target.checked)} />
        {label}
      </label>
      {hint && <p className="ml-6 text-xs text-gray-500">{hint}</p>}
    </div>
  );
}

/** Text input that commits on blur or Enter (autosave without a keystroke storm). */
function TextField({
  label,
  value,
  onCommit,
  hint,
  placeholder,
  mono,
  password,
  width,
  numeric,
  multiline,
}: {
  label: string;
  value: string;
  onCommit: (v: string) => void;
  hint?: string;
  placeholder?: string;
  mono?: boolean;
  password?: boolean;
  width?: string;
  numeric?: boolean;
  multiline?: boolean;
}) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  const commit = () => {
    if (v !== value) onCommit(v);
  };
  const cls = `input ${mono ? "font-mono text-xs" : ""} ${width ?? ""}`;
  return (
    <div>
      <label className="label">{label}</label>
      {multiline ? (
        <textarea className={`${cls} min-h-[72px]`} value={v} placeholder={placeholder} onChange={(e) => setV(e.target.value)} onBlur={commit} />
      ) : (
        <input
          className={cls}
          type={password ? "password" : "text"}
          value={v}
          placeholder={placeholder}
          onChange={(e) => setV(numeric ? e.target.value.replace(/\D/g, "") : e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      )}
      {hint && <p className="mt-1 text-xs text-gray-500">{hint}</p>}
    </div>
  );
}

// ---------------------------------------------------------------- General

function GeneralTab({ s, save }: { s: Settings; save: Save }) {
  const [zoom, setZoomState] = useState(zoomLevel());
  useEffect(() => onZoom(setZoomState), []);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  useEffect(() => {
    autostartEnabled().then(setAutostart).catch(() => setAutostart(null));
  }, []);
  const [startIn, setStartInState] = useState<StartIn>(getStartIn());

  return (
    <div className="space-y-5 text-sm">
      <Toggle v={s.closeToTray} on={(v) => void save({ closeToTray: v })} label="Keep running in the system tray when the window is closed" hint="Quit from the tray icon's menu." />
      {autostart !== null && (
        <Toggle
          v={autostart}
          on={(v) => {
            setAutostart(v);
            (v ? autostartEnable() : autostartDisable()).catch(() => setAutostart(!v));
          }}
          label="Start when I log in"
        />
      )}
      <div>
        <label className="label">Open the app on</label>
        <div className="flex gap-2">
          {(["last", "chat", "mail"] as StartIn[]).map((k) => (
            <button
              key={k}
              className={`btn text-xs ${startIn === k ? "border-blue-600 bg-blue-50 text-blue-900" : ""}`}
              onClick={() => {
                setStartIn(k);
                setStartInState(k);
              }}
            >
              {k === "last" ? "Last used" : k === "chat" ? "Chat" : "Mail"}
            </button>
          ))}
        </div>
        <p className="mt-1 text-xs text-gray-500">Drag the Mail and Chat buttons on the left rail to reorder them.</p>
      </div>
      <div className="flex items-center gap-2">
        <span>Text size</span>
        <button className="btn text-xs" onClick={() => void zoomStep(-1)} title="Ctrl -">A-</button>
        <span className="w-12 text-center text-xs text-gray-600">{Math.round(zoom * 100)}%</span>
        <button className="btn text-xs" onClick={() => void zoomStep(1)} title="Ctrl +">A+</button>
        <button className="btn btn-ghost text-xs" onClick={() => void setZoom(1)} title="Ctrl 0">Reset</button>
        <span className="text-xs text-gray-500">Ctrl + / Ctrl - / Ctrl 0, or Ctrl + mouse wheel</span>
      </div>
      <Toggle v={s.notifications} on={(v) => void save({ notifications: v })} label="Desktop notifications for new mail and chat messages" />
      <Toggle v={s.notificationSound} on={(v) => void save({ notificationSound: v })} label="Play a sound" />
      <div className="ml-6 flex flex-wrap items-center gap-x-6 gap-y-2">
        <SoundPicker label="New mail" value={asSoundName(s.soundMail, "chime")} onChange={(v) => void save({ soundMail: v })} />
        <SoundPicker label="Chat message" value={asSoundName(s.soundChat, "pop")} onChange={(v) => void save({ soundChat: v })} />
      </div>
    </div>
  );
}

function SoundPicker({ label, value, onChange }: { label: string; value: SoundName; onChange: (v: SoundName) => void }) {
  return (
    <label className="flex items-center gap-2 text-sm">
      {label}
      <select
        className="input w-auto"
        value={value}
        onChange={(e) => {
          const v = asSoundName(e.target.value, value);
          onChange(v);
          void playNamed(v);
        }}
      >
        {SOUND_NAMES.map((n) => (
          <option key={n} value={n}>
            {SOUNDS[n].label}
          </option>
        ))}
      </select>
      <button className="btn btn-ghost text-xs" onClick={() => void playNamed(value)} title="Preview">
        ▶
      </button>
    </label>
  );
}

// ------------------------------------------------------------- Appearance

function AppearanceTab() {
  const [p, setP] = useState(themePrefs());
  useEffect(() => onTheme(setP), []);
  const [json, setJson] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [logos, setLogos] = useState(getSenderLogos());
  const tokenValue = (k: string) => p.custom?.tokens[k] ?? getComputedStyle(document.documentElement).getPropertyValue(k).trim();

  const setToken = (k: string, v: string) => {
    const base = p.custom ?? { name: "My theme", base: p.mode === "system" ? "light" : p.mode, tokens: {} };
    setTheme({ custom: { ...base, tokens: { ...base.tokens, [k]: v } } });
  };

  return (
    <div className="space-y-6 text-sm">
      <div>
        <label className="label">Theme</label>
        <div className="flex flex-wrap gap-2">
          {(["light", "dark", "contrast", "system"] as Mode[]).map((m) => (
            <button
              key={m}
              className={`btn text-xs ${p.mode === m && !p.custom ? "border-blue-600 bg-blue-50 text-blue-900" : ""}`}
              onClick={() => setTheme({ mode: m, custom: null })}
            >
              {m === "system" ? "Follow system" : m === "contrast" ? "High contrast" : m[0].toUpperCase() + m.slice(1)}
            </button>
          ))}
        </div>
      </div>
      <div>
        <label className="label">Accent</label>
        <div className="flex gap-2">
          {ACCENTS.map((a) => (
            <button
              key={a}
              title={a}
              onClick={() => setTheme({ accent: a as Accent })}
              className={`h-7 w-7 rounded-full ring-2 ring-offset-2 ring-offset-gray-50 ${p.accent === a ? "ring-gray-900" : "ring-transparent"}`}
              style={{ background: ACCENT_SWATCH[a as Accent] }}
            />
          ))}
        </div>
      </div>
      <div className="flex gap-6">
        <div>
          <label className="label">Density</label>
          <select className="input w-auto" value={p.density} onChange={(e) => setTheme({ density: e.target.value as Density })}>
            <option value="comfortable">Comfortable</option>
            <option value="compact">Compact</option>
          </select>
        </div>
        <div>
          <label className="label">Font</label>
          <select className="input w-auto" value={p.font} onChange={(e) => setTheme({ font: e.target.value as FontChoice })}>
            <option value="system">System</option>
            <option value="inter">Inter (if installed)</option>
            <option value="serif">Serif</option>
            <option value="mono">Monospace</option>
          </select>
        </div>
        <div>
          <label className="label">HTML mail in dark mode</label>
          <select className="input w-auto" value={p.mailDark} onChange={(e) => setTheme({ mailDark: e.target.value as "paper" | "invert" })}>
            <option value="paper">Keep white paper</option>
            <option value="invert">Invert colours</option>
          </select>
        </div>
      </div>

      <Toggle
        v={logos}
        on={(v) => {
          setSenderLogos(v);
          setLogos(v);
        }}
        label="Show sender logos in mail (BIMI, then Gravatar, then the sender's site icon)"
        hint="Looks up BIMI via DNS, then gravatar.com and icons.duckduckgo.com; off keeps everything local. Takes effect on the next list refresh."
      />

      <details className="rounded-md border border-gray-200 p-3">
        <summary className="cursor-pointer text-sm font-medium">
          Custom theme {p.custom ? `— ${p.custom.name}` : ""}
        </summary>
        <p className="mt-2 text-xs text-gray-500">Changes apply live. Start from the current palette, tweak tokens, then export the JSON to share it.</p>
        <div className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2">
          {THEME_TOKENS.map((t) => (
            <label key={t.key} className="flex items-center justify-between gap-2 text-xs">
              <span className="text-gray-600">{t.label}</span>
              {t.key === "--radius" ? (
                <input className="input w-24 font-mono" defaultValue={tokenValue(t.key)} onBlur={(e) => setToken(t.key, e.target.value)} />
              ) : (
                <span className="flex items-center gap-1">
                  <input type="color" value={toHex(tokenValue(t.key))} onChange={(e) => setToken(t.key, e.target.value)} />
                  <span className="w-16 font-mono text-[10px] text-gray-500">{toHex(tokenValue(t.key))}</span>
                </span>
              )}
            </label>
          ))}
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <input className="input w-48 text-xs" placeholder="Theme name" value={p.custom?.name ?? ""} onChange={(e) => p.custom && setTheme({ custom: { ...p.custom, name: e.target.value } })} disabled={!p.custom} />
          <button className="btn text-xs" onClick={() => { setJson(exportThemeJson(p)); setMsg("Exported below — copy it anywhere."); }}>Export JSON</button>
          <button
            className="btn text-xs"
            onClick={() => {
              try {
                importThemeJson(json);
                setMsg("Theme imported.");
              } catch (e) {
                setMsg(`Could not import: ${errorMessage(e)}`);
              }
            }}
            disabled={!json.trim()}
          >
            Import JSON
          </button>
          <button className="btn text-xs" onClick={() => setTheme({ custom: null })} disabled={!p.custom}>
            Reset to built-in
          </button>
        </div>
        <textarea className="input mt-2 min-h-[120px] font-mono text-[11px]" placeholder='Paste a theme JSON here to import, or press "Export JSON".' value={json} onChange={(e) => setJson(e.target.value)} />
        {msg && <div className="mt-1 text-xs text-gray-600">{msg}</div>}
      </details>
    </div>
  );
}

const ACCENT_SWATCH: Record<Accent, string> = {
  blue: "#2563eb",
  indigo: "#4f46e5",
  violet: "#7c3aed",
  teal: "#0d9488",
  emerald: "#059669",
  rose: "#e11d48",
  orange: "#ea580c",
};

function toHex(v: string): string {
  const s = v.trim();
  if (/^#[0-9a-f]{6}$/i.test(s)) return s.toLowerCase();
  if (/^#[0-9a-f]{3}$/i.test(s)) return ("#" + s.slice(1).split("").map((c) => c + c).join("")).toLowerCase();
  const m = s.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
  if (m) return "#" + [m[1], m[2], m[3]].map((n) => Number(n).toString(16).padStart(2, "0")).join("");
  return "#888888";
}

// ------------------------------------------------------------------ Mail

function MailTab({ s, save }: { s: Settings; save: Save }) {
  return (
    <div className="space-y-5 text-sm">
      <Toggle v={s.conversationView} on={(v) => void save({ conversationView: v })} label="Conversation view (group replies into threads)" />
      <Toggle v={s.mailShowImages} on={(v) => void save({ mailShowImages: v })} label="Always show remote images" hint="Off (the default) means senders can't tell when you open a message; you can still show images per message." />
      <div className="flex gap-6">
        <TextField label="Undo send window (seconds)" value={String(s.undoSendSeconds)} numeric width="w-28" onCommit={(v) => void save({ undoSendSeconds: Math.max(0, Number(v) || 0) })} hint="0 sends immediately. Max 60." />
        <TextField label="Check for new mail every (seconds)" value={String(s.mailPollSeconds)} numeric width="w-28" onCommit={(v) => void save({ mailPollSeconds: Math.max(0, Number(v) || 0) })} hint="Minimum 15; 0 turns background checks off." />
      </div>
      <TextField label="Signature (all accounts unless overridden under Accounts)" value={s.mailSignature} multiline onCommit={(v) => void save({ mailSignature: v })} />
      <TemplatesEditor value={s.mailTemplates} onCommit={(v) => void save({ mailTemplates: v })} />
      <details className="text-xs text-gray-600">
        <summary className="cursor-pointer">Keyboard shortcuts</summary>
        <div className="mt-1 grid grid-cols-2 gap-x-6 gap-y-0.5 font-mono">
          <span>j / k</span><span className="font-sans">next / previous</span>
          <span>e</span><span className="font-sans">archive</span>
          <span># or Del</span><span className="font-sans">trash</span>
          <span>!</span><span className="font-sans">spam</span>
          <span>r / a / f</span><span className="font-sans">reply / reply all / forward</span>
          <span>c</span><span className="font-sans">compose</span>
          <span>s</span><span className="font-sans">star</span>
          <span>x / *</span><span className="font-sans">select / select all</span>
          <span>Shift+U / Shift+I</span><span className="font-sans">mark unread / read</span>
          <span>/</span><span className="font-sans">search</span>
          <span>u or Esc</span><span className="font-sans">back to list</span>
        </div>
      </details>
    </div>
  );
}

function TemplatesEditor({ value, onCommit }: { value: string; onCommit: (v: string) => void }) {
  const [text, setText] = useState(value || "[]");
  return (
    <div>
      <div className="mb-1 text-sm">Compose templates (JSON)</div>
      <p className="mb-1 text-xs text-gray-500">Array of {`{ "name", "subject", "body" }`}. Available in the compose window.</p>
      <textarea
        className="input min-h-[100px] font-mono text-[11px]"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          try {
            JSON.parse(text || "[]");
            onCommit(text.trim() || "[]");
          } catch {
            /* leave unsaved until it is valid JSON */
          }
        }}
      />
    </div>
  );
}

// -------------------------------------------------------------- Accounts

function AccountsTab({ s, save }: { s: Settings; save: Save }) {
  const { accounts, addAccount, addImap, removeAccount, busy } = useMail();
  const [confirmId, setConfirmId] = useState<number | null>(null);
  const [imap, setImap] = useState({ host: "", username: "", password: "", port: "993", smtpPort: "587" });
  return (
    <div className="space-y-6 text-sm">
      <div>
        <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">Google OAuth client</div>
        <p className="mb-3 text-gray-600">
          This app ships no Google credentials. Create an OAuth client of type <strong>Desktop app</strong> in the Google Cloud console, enable the Gmail API, and paste the client ID and secret here (see the README). The secret is stored in the OS credential store.
        </p>
        <div className="space-y-3">
          <TextField label="Client ID" value={s.googleClientId} mono placeholder="xxxxxxxx.apps.googleusercontent.com" onCommit={(v) => void save({ googleClientId: v })} />
          <TextField
            label={`Client secret ${s.googleClientSecretSet ? "(set — enter a new one to replace, blank to clear)" : ""}`}
            value=""
            mono
            password
            placeholder={s.googleClientSecretSet ? "••••••••" : "GOCSPX-…"}
            onCommit={(v) => void save({ googleClientSecret: v })}
          />
        </div>
      </div>

      <div>
        <div className="mb-2 flex items-center justify-between">
          <div className="text-xs font-semibold uppercase tracking-wide text-gray-500">Connected accounts</div>
          <button className="btn text-xs" onClick={() => void addAccount()} disabled={busy}>
            {busy ? "Waiting for browser…" : "+ Connect Gmail"}
          </button>
        </div>
        <p className="mb-3 text-xs text-gray-500">
          Google Workspace teams can mark the OAuth app Internal to skip verification. A public Gmail app needs Google’s restricted-scope review, or use IMAP below.
        </p>
        <div className="mb-4 rounded-md border border-gray-200 bg-gray-50 p-3">
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">IMAP (any provider)</div>
          <div className="grid grid-cols-2 gap-2">
            <input className="input col-span-2" placeholder="imap.example.com" value={imap.host} onChange={(e) => setImap({ ...imap, host: e.target.value })} />
            <input className="input col-span-2" placeholder="you@example.com" value={imap.username} onChange={(e) => setImap({ ...imap, username: e.target.value })} />
            <input className="input col-span-2" type="password" placeholder="Password or app password" value={imap.password} onChange={(e) => setImap({ ...imap, password: e.target.value })} />
            <input className="input" placeholder="IMAP port" value={imap.port} onChange={(e) => setImap({ ...imap, port: e.target.value })} />
            <input className="input" placeholder="SMTP port" value={imap.smtpPort} onChange={(e) => setImap({ ...imap, smtpPort: e.target.value })} />
          </div>
          <button
            className="btn mt-2 text-xs"
            disabled={busy || !imap.host.trim() || !imap.username.trim() || !imap.password}
            onClick={() =>
              void addImap({
                host: imap.host.trim(),
                username: imap.username.trim(),
                password: imap.password,
                port: Number(imap.port) || 993,
                smtpHost: imap.host.trim(),
                smtpPort: Number(imap.smtpPort) || 587,
              }).then(() => setImap({ host: "", username: "", password: "", port: "993", smtpPort: "587" }))
            }
          >
            Connect IMAP
          </button>
        </div>
        {accounts.length === 0 && <div className="text-gray-500">No accounts yet.</div>}
        <div className="space-y-3">
          {accounts.map((a) => (
            <div key={a.id} className="rounded-md border border-gray-200 bg-white p-3">
              <div className="flex items-center gap-2">
                <span className="font-medium">{a.email}</span>
                <span className="rounded bg-gray-100 px-1.5 text-[10px] uppercase text-gray-600">{a.provider}</span>
                <div className="flex-1" />
                {a.provider === "gmail" && (
                <button className="btn text-xs" onClick={() => mail.reauth(a.id).catch(() => undefined)} title="Re-run the Google consent flow (needed once for filter management on older accounts)">
                  Sign in again
                </button>
                )}
                {confirmId === a.id ? (
                  <span className="flex items-center gap-1">
                    <span className="text-xs text-red-700">Remove {a.email} and its cached mail?</span>
                    <button className="btn btn-danger text-xs" onClick={() => { setConfirmId(null); void removeAccount(a.id); }}>Remove</button>
                    <button className="btn btn-ghost text-xs" onClick={() => setConfirmId(null)}>Cancel</button>
                  </span>
                ) : (
                  <button className="btn btn-ghost btn-danger text-xs" onClick={() => setConfirmId(a.id)}>
                    Remove
                  </button>
                )}
              </div>
              <div className="mt-2">
                <AccountSignature account={a} value={s.accountSignatures[a.id]} save={save} />
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function AccountSignature({ account, value, save }: { account: Account; value: string | undefined; save: Save }) {
  const overridden = value !== undefined;
  return (
    <div>
      <label className="flex items-center gap-2 text-xs">
        <input
          type="checkbox"
          checked={overridden}
          onChange={(e) => void save({ accountSignatures: { [account.id]: e.target.checked ? "" : null } })}
        />
        Use a signature specific to this account
      </label>
      {overridden && (
        <TextField label="" value={value ?? ""} multiline placeholder="Leave empty for no signature on this account" onCommit={(v) => void save({ accountSignatures: { [account.id]: v } })} />
      )}
    </div>
  );
}

// --------------------------------------------------------------- Filters

function criteriaKey(f: MailFilter) {
  return JSON.stringify([f.criteria, [...f.addLabelIds].sort(), [...f.removeLabelIds].sort()]);
}

function FiltersTab() {
  const accounts = useMail((m) => m.accounts);
  const labels = useMail((m) => m.labels);
  const openFilterEditor = useMail((m) => m.openFilterEditor);
  const filterEditor = useMail((m) => m.filterEditor);
  const setAccount = useMail((m) => m.setAccount);
  const [byAccount, setByAccount] = useState<Record<number, MailFilter[] | string>>({});
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmDel, setConfirmDel] = useState<string | null>(null);

  const load = useCallback((a: Account) => {
    mail
      .listFilters(a.id)
      .then((f) => setByAccount((prev) => ({ ...prev, [a.id]: f })))
      .catch((e) => setByAccount((prev) => ({ ...prev, [a.id]: errorMessage(e) })));
  }, []);
  useEffect(() => {
    for (const a of accounts) load(a);
  }, [accounts, load]);
  useEffect(() => {
    if (!filterEditor) for (const a of accounts) load(a);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterEditor]);

  const edit = (a: Account, f: MailFilter) => {
    const pick = (k: string) => f.criteria.find(([key]) => key === k)?.[1] ?? "";
    const userLabel = f.addLabelIds.find((id) => labels.some((l) => l.remoteId === id && l.kind === "user")) ?? null;
    const prefill: Partial<NewFilter> = {
      from: pick("from"),
      to: pick("to"),
      subject: pick("subject"),
      hasWords: pick("has the words"),
      notWords: pick("doesn't have"),
      hasAttachment: f.criteria.some(([k]) => k === "has attachment"),
      skipInbox: f.removeLabelIds.includes("INBOX"),
      markRead: f.removeLabelIds.includes("UNREAD"),
      star: f.addLabelIds.includes("STARRED"),
      delete: f.addLabelIds.includes("TRASH"),
      neverSpam: f.removeLabelIds.includes("SPAM"),
      markImportant: f.addLabelIds.includes("IMPORTANT"),
      addLabel: userLabel,
      applyToExisting: false,
    };
    setAccount(a.id);
    openFilterEditor(prefill, { accountId: a.id, filterId: f.id });
  };

  const remove = async (a: Account, f: MailFilter) => {
    setBusy(f.id);
    try {
      await mail.deleteFilter(a.id, f.id);
      load(a);
    } catch (e) {
      setByAccount((prev) => ({ ...prev, [a.id]: errorMessage(e) }));
    } finally {
      setBusy(null);
      setConfirmDel(null);
    }
  };

  if (accounts.length === 0) return <div className="text-sm text-gray-500">Connect an account first.</div>;

  return (
    <div className="space-y-6 text-sm">
      <p className="text-gray-600">Rules your provider applies to incoming mail. Accounts connected before filter management was added need one "Sign in again" (Accounts tab).</p>
      <input className="input" placeholder="Search filters (sender, words, label)…" value={q} onChange={(e) => setQ(e.target.value)} />
      {accounts.map((a) => {
        const list = byAccount[a.id];
        const items = Array.isArray(list) ? list : [];
        const seen = new Map<string, number>();
        for (const f of items) seen.set(criteriaKey(f), (seen.get(criteriaKey(f)) ?? 0) + 1);
        const dupes = items.filter((f) => (seen.get(criteriaKey(f)) ?? 0) > 1);
        const needle = q.trim().toLowerCase();
        const shown = items.filter((f) => !needle || JSON.stringify([f.criteria, f.addLabels, f.removeLabels]).toLowerCase().includes(needle));
        return (
          <div key={a.id}>
            <div className="mb-1 flex items-center gap-2">
              <span className="font-medium">{a.email}</span>
              <span className="text-xs text-gray-500">{items.length} filter{items.length === 1 ? "" : "s"}</span>
              <button
                className="btn btn-ghost text-xs"
                onClick={() => {
                  setAccount(a.id);
                  openFilterEditor();
                }}
              >
                + Create filter
              </button>
            </div>
            {dupes.length > 0 && (
              <div className="mb-2 flex items-center gap-2 rounded-md border border-amber-200 bg-amber-50 px-3 py-1.5 text-xs text-amber-900">
                {dupes.length / 2 | 0 || 1} duplicate rule{dupes.length > 2 ? "s" : ""} found — identical filters do nothing extra.
                <button
                  className="ml-auto underline"
                  onClick={async () => {
                    const keep = new Set<string>();
                    for (const f of items) {
                      const k = criteriaKey(f);
                      if (seen.get(k)! > 1 && keep.has(k)) await remove(a, f);
                      keep.add(k);
                    }
                  }}
                >
                  Delete the duplicates
                </button>
              </div>
            )}
            {list === undefined && <div className="text-xs text-gray-500">Loading…</div>}
            {typeof list === "string" && <div className="text-xs text-red-700">{list}</div>}
            {Array.isArray(list) && shown.length === 0 && <div className="text-xs text-gray-500">{items.length ? "No filters match." : "No filters."}</div>}
            {shown.length > 0 && (
              <ul className="divide-y divide-gray-100 rounded-md border border-gray-200 bg-white text-xs">
                {shown.map((f) => {
                  const dup = (seen.get(criteriaKey(f)) ?? 0) > 1;
                  return (
                    <li key={f.id} className={`flex flex-wrap items-baseline gap-x-3 gap-y-1 px-3 py-2 ${dup ? "bg-amber-50/60" : ""}`}>
                      <span className="text-gray-700">
                        {f.criteria.length === 0
                          ? "all mail"
                          : f.criteria.map(([k, v]) => (
                              <span key={k} className="mr-2">
                                <span className="text-gray-500">{k}:</span> <span className="font-mono">{v}</span>
                              </span>
                            ))}
                      </span>
                      <span className="text-gray-400">→</span>
                      <span className="flex-1">
                        {f.addLabels.map((l) => (
                          <span key={`+${l}`} className="mr-1 rounded bg-green-50 px-1 text-green-800">+{l}</span>
                        ))}
                        {f.removeLabels.map((l) => (
                          <span key={`-${l}`} className="mr-1 rounded bg-red-50 px-1 text-red-800">−{l}</span>
                        ))}
                        {f.forward && <span className="rounded bg-blue-50 px-1 text-blue-800">forward to {f.forward}</span>}
                        {dup && <span className="ml-1 rounded bg-amber-100 px-1 text-amber-900">duplicate</span>}
                      </span>
                      {busy === f.id ? (
                        <Spinner size={12} className="text-blue-600" />
                      ) : confirmDel === f.id ? (
                        <span className="flex items-center gap-1">
                          <button className="text-red-700 hover:underline" onClick={() => void remove(a, f)}>confirm delete</button>
                          <button className="text-gray-500 hover:underline" onClick={() => setConfirmDel(null)}>cancel</button>
                        </span>
                      ) : (
                        <span className="flex items-center gap-2">
                          <button className="text-blue-700 hover:underline" onClick={() => edit(a, f)}>edit</button>
                          <button className="text-red-700 hover:underline" onClick={() => setConfirmDel(f.id)}>delete</button>
                        </span>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ------------------------------------------------------------------ Chat

function ChatTab({ s, save }: { s: Settings; save: Save }) {
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = () => chatIpc.storageStats().then(setStats).catch(() => setStats(null));
  useEffect(() => {
    void load();
  }, []);
  const clear = async (which: "received" | "outgoing", label: string) => {
    if (!confirm(`Delete all ${label}? Messages stay, but their files will be gone.`)) return;
    const n = await chatIpc.clearStorage(which);
    setMsg(`${n} file${n === 1 ? "" : "s"} deleted.`);
    void load();
  };
  return (
    <div className="space-y-5 text-sm">
      <TextField label="Display name (what peers see)" value={s.chatDisplayName} onCommit={(v) => void save({ chatDisplayName: v })} />
      <TextField label="Listen port" value={String(s.chatPort)} numeric width="w-32" onCommit={(v) => void save({ chatPort: Number(v) || undefined }, "Port changes apply after a restart.")} hint="Peers connect to this port; allow it through your firewall." />
      <div>
        <label className="label">Received files folder</label>
        <div className="flex gap-2">
          <TextField label="" value={s.chatDownloadDir} placeholder="Default: Downloads/Trakzen Conecta" onCommit={(v) => void save({ chatDownloadDir: v })} />
          <button
            className="btn self-start"
            onClick={async () => {
              const picked = await open({ directory: true, title: "Choose folder" });
              if (typeof picked === "string") void save({ chatDownloadDir: picked });
            }}
          >
            Browse
          </button>
        </div>
      </div>
      {stats && (
        <div>
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">Storage</div>
          <div className="space-y-2">
            <div className="flex items-center gap-3">
              <div className="flex-1">
                <div>Received files — {stats.receivedFiles} file{stats.receivedFiles === 1 ? "" : "s"}, {bytes(stats.receivedBytes)}</div>
                <div className="truncate font-mono text-[11px] text-gray-500">{stats.receivedDir}</div>
              </div>
              <button className="btn text-xs" disabled={stats.receivedFiles === 0} onClick={() => void clear("received", "received files")}>Clear</button>
            </div>
            <div className="flex items-center gap-3">
              <div className="flex-1">
                <div>Pasted &amp; dropped media — {stats.outgoingFiles} file{stats.outgoingFiles === 1 ? "" : "s"}, {bytes(stats.outgoingBytes)}</div>
                <div className="truncate font-mono text-[11px] text-gray-500">{stats.outgoingDir}</div>
              </div>
              <button className="btn text-xs" disabled={stats.outgoingFiles === 0} onClick={() => void clear("outgoing", "pasted media")}>Clear</button>
            </div>
            {msg && <div className="text-xs text-green-700">{msg}</div>}
          </div>
        </div>
      )}
    </div>
  );
}

// ----------------------------------------------------------------- About

function AboutTab() {
  const built = useMemo(() => new Date(__BUILD_DATE__.replace(" UTC", "Z").replace(" ", "T")).toLocaleString(), []);
  return (
    <div className="space-y-2 text-sm text-gray-600">
      <div>
        Trakzen Conecta <span className="font-mono">v{__APP_VERSION__}</span> · build <span className="font-mono">{__BUILD_ID__}</span>
      </div>
      <div>
        {__BUILD_DATE__} · {built}
      </div>
      <div className="flex gap-3 text-xs">
        <button className="text-blue-700 hover:underline" onClick={() => void openUrl("https://github.com/nabeelnaeem/trakzen-conecta/releases")}>Releases</button>
        <button className="text-blue-700 hover:underline" onClick={() => void openUrl("https://github.com/nabeelnaeem/trakzen-conecta/issues")}>Report a problem</button>
        <button className="text-blue-700 hover:underline" onClick={() => void openUrl("https://github.com/nabeelnaeem/trakzen-conecta/blob/main/PRIVACY.md")}>Privacy</button>
      </div>
      <p className="pt-2 text-xs">MIT licensed. Mail is cached locally in SQLite; OAuth tokens and the client secret live in the OS credential store.</p>
    </div>
  );
}

