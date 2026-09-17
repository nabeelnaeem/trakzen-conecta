import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { errorMessage, mail, settings } from "../../lib/ipc";
import { asSoundName, notifyPrefs, playNamed, SOUND_NAMES, SOUNDS, type SoundName } from "../../lib/notify";
import { bytes } from "../../lib/format";
import { chat as chatIpc } from "../../lib/ipc";
import type { StorageStats } from "../../lib/types";
import { onZoom, setZoom, zoomLevel, zoomStep } from "../../lib/zoom";
import { openUrl } from "@tauri-apps/plugin-opener";
import { disable as autostartDisable, enable as autostartEnable, isEnabled as autostartEnabled } from "@tauri-apps/plugin-autostart";
import type { Account, MailFilter, SettingsView as Settings } from "../../lib/types";
import { useChat } from "../chat/store";
import { useMail } from "../mail/store";

export function SettingsView() {
  const [s, setS] = useState<Settings | null>(null);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [name, setName] = useState("");
  const [port, setPort] = useState("");
  const [dir, setDir] = useState("");
  const [showImages, setShowImages] = useState(true);
  const [signature, setSignature] = useState("");
  const [poll, setPoll] = useState("60");
  const [closeToTray, setCloseToTray] = useState(true);
  const [notifications, setNotifications] = useState(true);
  const [sound, setSound] = useState(true);
  const [soundMail, setSoundMail] = useState<SoundName>("chime");
  const [soundChat, setSoundChat] = useState<SoundName>("pop");
  const [conversation, setConversation] = useState(true);
  const [undo, setUndo] = useState("10");
  const [zoom, setZoomState] = useState(zoomLevel());
  useEffect(() => onZoom(setZoomState), []);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  useEffect(() => {
    autostartEnabled().then(setAutostart).catch(() => setAutostart(null));
  }, []);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const refreshIdentity = useChat((c) => c.refreshIdentity);
  const applyMail = useMail((m) => m.applySettings);

  useEffect(() => {
    settings
      .get()
      .then((v) => {
        setS(v);
        setClientId(v.googleClientId);
        setName(v.chatDisplayName);
        setPort(String(v.chatPort));
        setDir(v.chatDownloadDir);
        setShowImages(v.mailShowImages);
        setSignature(v.mailSignature);
        setPoll(String(v.mailPollSeconds));
        setCloseToTray(v.closeToTray);
        setNotifications(v.notifications);
        setSound(v.notificationSound);
        setSoundMail(asSoundName(v.soundMail, "chime"));
        setSoundChat(asSoundName(v.soundChat, "pop"));
        setConversation(v.conversationView);
        setUndo(String(v.undoSendSeconds));
      })
      .catch((e) => setErr(errorMessage(e)));
  }, []);

  const save = async () => {
    setMsg(null);
    setErr(null);
    try {
      const v = await settings.update({
        googleClientId: clientId,
        ...(clientSecret ? { googleClientSecret: clientSecret } : {}),
        chatDisplayName: name,
        chatPort: Number(port) || undefined,
        chatDownloadDir: dir,
        mailShowImages: showImages,
        mailSignature: signature,
        mailPollSeconds: Math.max(0, Number(poll) || 0),
        closeToTray,
        notifications,
        notificationSound: sound,
        soundMail,
        soundChat,
        conversationView: conversation,
        undoSendSeconds: Math.max(0, Number(undo) || 0),
      });
      setS(v);
      setClientSecret("");
      applyMail({ showImages: v.mailShowImages, conversations: v.conversationView, undoSeconds: v.undoSendSeconds });
      notifyPrefs.notifications = v.notifications;
      notifyPrefs.sound = v.notificationSound;
      notifyPrefs.mail = asSoundName(v.soundMail, "chime");
      notifyPrefs.chat = asSoundName(v.soundChat, "pop");
      setMsg("Saved." + (Number(port) !== s?.chatPort ? " Port changes apply after restart." : ""));
      void refreshIdentity();
    } catch (e) {
      setErr(errorMessage(e));
    }
  };

  if (!s) return <div className="p-6 text-sm text-gray-500">{err ?? "Loading…"}</div>;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl space-y-8 p-6">
        <section>
          <h2 className="text-base font-semibold">General</h2>
          <div className="mt-3 space-y-2 text-sm">
            <Toggle v={closeToTray} on={setCloseToTray} label="Keep running in the system tray when the window is closed" hint="Quit from the tray icon's menu." />
            {autostart !== null && (
              <Toggle
                v={autostart}
                on={(v) => {
                  setAutostart(v);
                  (v ? autostartEnable() : autostartDisable()).catch((e) => setErr(errorMessage(e)));
                }}
                label="Start when I log in"
                hint="Applies immediately; no need to press Save."
              />
            )}
            <Toggle v={notifications} on={setNotifications} label="Desktop notifications for new mail and chat messages" />
            <div className="flex items-center gap-2">
              <span className="text-sm">Text size</span>
              <button className="btn text-xs" onClick={() => void zoomStep(-1)} title="Ctrl -">A-</button>
              <span className="w-12 text-center text-xs text-gray-600">{Math.round(zoom * 100)}%</span>
              <button className="btn text-xs" onClick={() => void zoomStep(1)} title="Ctrl +">A+</button>
              <button className="btn btn-ghost text-xs" onClick={() => void setZoom(1)} title="Ctrl 0">Reset</button>
              <span className="text-xs text-gray-500">Ctrl + / Ctrl - / Ctrl 0, or Ctrl + mouse wheel</span>
            </div>
            <Toggle v={sound} on={setSound} label="Play a sound" />
            <div className="ml-6 flex flex-wrap items-center gap-x-6 gap-y-2">
              <SoundPicker label="New mail" value={soundMail} onChange={setSoundMail} />
              <SoundPicker label="Chat message" value={soundChat} onChange={setSoundChat} />
            </div>
          </div>
        </section>

        <section>
          <h2 className="text-base font-semibold">Mail</h2>
          <div className="mt-3 space-y-3 text-sm">
            <Toggle v={conversation} on={setConversation} label="Conversation view (group replies into threads)" />
            <Toggle v={showImages} on={setShowImages} label="Always show remote images" hint="Off means senders can't tell when you open a message; you can still show images per message." />
            <div className="flex gap-6">
              <div>
                <label className="label">Undo send window (seconds)</label>
                <input className="input w-28" value={undo} onChange={(e) => setUndo(e.target.value.replace(/\D/g, ""))} />
                <p className="mt-1 text-xs text-gray-500">0 sends immediately. Max 60.</p>
              </div>
              <div>
                <label className="label">Check for new mail every (seconds)</label>
                <input className="input w-28" value={poll} onChange={(e) => setPoll(e.target.value.replace(/\D/g, ""))} />
                <p className="mt-1 text-xs text-gray-500">Minimum 15; 0 turns background checks off.</p>
              </div>
            </div>
            <div>
              <label className="label">Signature (appended to messages you send)</label>
              <textarea className="input min-h-[72px]" value={signature} onChange={(e) => setSignature(e.target.value)} />
            </div>
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
        </section>

        <section>
          <h2 className="text-base font-semibold">Gmail</h2>
          <p className="mt-1 text-sm text-gray-600">
            This app does not ship with Google credentials. Create an OAuth client of type{" "}
            <strong>Desktop app</strong> in the Google Cloud console, enable the Gmail API, and
            paste the client ID and secret here. See the README for a walkthrough.
          </p>
          <div className="mt-3 space-y-3">
            <div>
              <label className="label">Client ID</label>
              <input className="input font-mono text-xs" value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="xxxxxxxx.apps.googleusercontent.com" />
            </div>
            <div>
              <label className="label">
                Client secret {s.googleClientSecretSet && <span className="text-green-700">(set — leave blank to keep)</span>}
              </label>
              <input className="input font-mono text-xs" type="password" value={clientSecret} onChange={(e) => setClientSecret(e.target.value)} placeholder="GOCSPX-…" />
            </div>
          </div>
        </section>

        <section>
          <h2 className="text-base font-semibold">Chat</h2>
          <div className="mt-3 space-y-3">
            <div>
              <label className="label">Display name (what peers see)</label>
              <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div>
              <label className="label">Listen port</label>
              <input className="input w-32" value={port} onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))} />
              <p className="mt-1 text-xs text-gray-500">
                Peers connect to this port. Allow it through your firewall. Changes take effect after restart.
              </p>
            </div>
            <div>
              <label className="label">Received files folder</label>
              <div className="flex gap-2">
                <input className="input" value={dir} onChange={(e) => setDir(e.target.value)} placeholder="Default: Downloads/Trakzen Conecta" />
                <button
                  className="btn"
                  onClick={async () => {
                    const picked = await open({ directory: true, title: "Choose folder" });
                    if (typeof picked === "string") setDir(picked);
                  }}
                >
                  Browse
                </button>
              </div>
            </div>
          </div>
        </section>

        <div className="flex items-center gap-3">
          <button className="btn btn-primary" onClick={() => void save()}>
            Save
          </button>
          {msg && <span className="text-sm text-green-700">{msg}</span>}
          {err && <span className="text-sm text-red-700">{err}</span>}
        </div>

        <FiltersSection />
        <StorageSection />

        <section className="border-t border-gray-200 pt-4 text-sm text-gray-600">
          <h2 className="text-base font-semibold text-gray-900">About</h2>
          <div className="mt-2">
            Trakzen Conecta <span className="font-mono">v{__APP_VERSION__}</span> · build{" "}
            <span className="font-mono" title="Build id">{__BUILD_ID__}</span> · {__BUILD_DATE__}
            {" · "}
            <span title="Local time">{new Date(__BUILD_DATE__.replace(" UTC", "Z").replace(" ", "T")).toLocaleString()}</span>
          </div>
          <div className="mt-1 flex gap-3 text-xs">
            <button className="text-blue-700 hover:underline" onClick={() => void openUrl("https://github.com/nabeelnaeem/trakzen-conecta/releases")}>
              Releases
            </button>
            <button className="text-blue-700 hover:underline" onClick={() => void openUrl("https://github.com/nabeelnaeem/trakzen-conecta/issues")}>
              Report a problem
            </button>
          </div>
        </section>
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

/** Media the chat has written to disk, with one-click clean-up. */
function StorageSection() {
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
  if (!stats) return null;
  return (
    <section>
      <h2 className="text-base font-semibold">Chat storage</h2>
      <p className="mt-1 text-sm text-gray-600">Deleting a message also removes its file; this clears everything at once.</p>
      <div className="mt-3 space-y-2 text-sm">
        <div className="flex items-center gap-3">
          <div className="flex-1">
            <div>Received files — {stats.receivedFiles} file{stats.receivedFiles === 1 ? "" : "s"}, {bytes(stats.receivedBytes)}</div>
            <div className="truncate font-mono text-[11px] text-gray-500">{stats.receivedDir}</div>
          </div>
          <button className="btn text-xs" disabled={stats.receivedFiles === 0} onClick={() => void clear("received", "received files")}>
            Clear
          </button>
        </div>
        <div className="flex items-center gap-3">
          <div className="flex-1">
            <div>Pasted &amp; dropped media — {stats.outgoingFiles} file{stats.outgoingFiles === 1 ? "" : "s"}, {bytes(stats.outgoingBytes)}</div>
            <div className="truncate font-mono text-[11px] text-gray-500">{stats.outgoingDir}</div>
          </div>
          <button className="btn text-xs" disabled={stats.outgoingFiles === 0} onClick={() => void clear("outgoing", "pasted media")}>
            Clear
          </button>
        </div>
        {msg && <div className="text-xs text-green-700">{msg}</div>}
      </div>
    </section>
  );
}

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

/** Server-side filter rules for each account: list, delete, create. */
function FiltersSection() {
  const accounts = useMail((m) => m.accounts);
  const openFilterEditor = useMail((m) => m.openFilterEditor);
  const filterEditor = useMail((m) => m.filterEditor);
  const setAccount = useMail((m) => m.setAccount);
  const [byAccount, setByAccount] = useState<Record<number, MailFilter[] | string>>({});
  const [reauthMsg, setReauthMsg] = useState<string | null>(null);

  const load = (a: Account) =>
    mail
      .listFilters(a.id)
      .then((f) => setByAccount((prev) => ({ ...prev, [a.id]: f })))
      .catch((e) => setByAccount((prev) => ({ ...prev, [a.id]: errorMessage(e) })));

  useEffect(() => {
    for (const a of accounts) void load(a);
  }, [accounts]);

  // Reload after the editor closes (a filter may have been created).
  useEffect(() => {
    if (!filterEditor) for (const a of accounts) void load(a);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterEditor]);

  if (accounts.length === 0) return null;

  return (
    <section>
      <h2 className="text-base font-semibold">Mail filters</h2>
      <p className="mt-1 text-sm text-gray-600">
        Rules the provider applies to incoming mail before it reaches this app. Accounts connected
        before filter management was added need to be signed in again once to grant the extra
        permission.
      </p>
      {reauthMsg && <div className="mt-2 text-xs text-green-700">{reauthMsg}</div>}
      {accounts.map((a) => (
        <div key={a.id} className="mt-3">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">{a.email}</span>
            <button
              className="btn btn-ghost text-xs"
              onClick={() => {
                setAccount(a.id);
                openFilterEditor();
              }}
            >
              + Create filter
            </button>
            <button
              className="btn btn-ghost text-xs"
              onClick={() =>
                mail
                  .reauth(a.id)
                  .then(() => {
                    setReauthMsg(`${a.email} signed in again.`);
                    void load(a);
                  })
                  .catch((e) => setByAccount((prev) => ({ ...prev, [a.id]: errorMessage(e) })))
              }
            >
              Sign in again
            </button>
          </div>
          <AccountFilters account={a} filters={byAccount[a.id]} onDeleted={() => void load(a)} />
        </div>
      ))}
    </section>
  );
}

function AccountFilters({ account, filters, onDeleted }: { account: Account; filters: MailFilter[] | string | undefined; onDeleted: () => void }) {
  const [err, setErr] = useState<string | null>(null);
  return (
    <div className="mt-1">
      {filters === undefined && <div className="text-xs text-gray-500">Loading…</div>}
      {typeof filters === "string" && <div className="text-xs text-red-700">{filters}</div>}
      {Array.isArray(filters) && filters.length === 0 && <div className="text-xs text-gray-500">No filters.</div>}
      {Array.isArray(filters) && filters.length > 0 && (
        <ul className="mt-1 divide-y divide-gray-100 rounded-md border border-gray-200 text-xs">
          {filters.map((f) => (
            <li key={f.id} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-3 py-2">
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
                  <span key={`+${l}`} className="mr-1 rounded bg-green-50 px-1 text-green-800">
                    +{l}
                  </span>
                ))}
                {f.removeLabels.map((l) => (
                  <span key={`-${l}`} className="mr-1 rounded bg-red-50 px-1 text-red-800">
                    −{l}
                  </span>
                ))}
                {f.forward && <span className="rounded bg-blue-50 px-1 text-blue-800">forward to {f.forward}</span>}
              </span>
              <button
                className="text-red-700 hover:underline"
                onClick={() => {
                  if (!confirm("Delete this filter?")) return;
                  mail.deleteFilter(account.id, f.id).then(onDeleted).catch((e) => setErr(errorMessage(e)));
                }}
              >
                delete
              </button>
            </li>
          ))}
        </ul>
      )}
      {err && <div className="mt-1 text-xs text-red-700">{err}</div>}
    </div>
  );
}
