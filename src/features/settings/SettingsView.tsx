import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { errorMessage, settings } from "../../lib/ipc";
import type { SettingsView as Settings } from "../../lib/types";
import { useChat } from "../chat/store";

export function SettingsView() {
  const [s, setS] = useState<Settings | null>(null);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [name, setName] = useState("");
  const [port, setPort] = useState("");
  const [dir, setDir] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const refreshIdentity = useChat((c) => c.refreshIdentity);

  useEffect(() => {
    settings
      .get()
      .then((v) => {
        setS(v);
        setClientId(v.googleClientId);
        setName(v.chatDisplayName);
        setPort(String(v.chatPort));
        setDir(v.chatDownloadDir);
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
      });
      setS(v);
      setClientSecret("");
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
      </div>
    </div>
  );
}
