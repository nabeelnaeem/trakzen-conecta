import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { errorMessage, mail, settings } from "../../lib/ipc";
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
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const refreshIdentity = useChat((c) => c.refreshIdentity);
  const setMailShowImages = useMail((m) => m.setShowImages);

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
      });
      setS(v);
      setClientSecret("");
      setMailShowImages(v.mailShowImages);
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
          <h2 className="text-base font-semibold">Mail</h2>
          <div className="mt-3 space-y-3">
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={showImages} onChange={(e) => setShowImages(e.target.checked)} />
              Always show remote images
            </label>
            <p className="-mt-2 text-xs text-gray-500">
              Off means senders can't tell when you open a message; you can still show images per message.
            </p>
            <div>
              <label className="label">Check for new mail every (seconds)</label>
              <input className="input w-32" value={poll} onChange={(e) => setPoll(e.target.value.replace(/\D/g, ""))} />
              <p className="mt-1 text-xs text-gray-500">
                Each check is one small request per account. Minimum 15; 0 turns background checks off
                (the app still syncs when you return to the window or press ⟳).
              </p>
            </div>
            <div>
              <label className="label">Signature (appended to messages you send)</label>
              <textarea className="input min-h-[72px]" value={signature} onChange={(e) => setSignature(e.target.value)} />
            </div>
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
      </div>
    </div>
  );
}

/** Read-only view of the server-side filter rules for each account. */
function FiltersSection() {
  const accounts = useMail((m) => m.accounts);
  const [byAccount, setByAccount] = useState<Record<number, MailFilter[] | string>>({});

  useEffect(() => {
    for (const a of accounts) {
      mail
        .listFilters(a.id)
        .then((f) => setByAccount((prev) => ({ ...prev, [a.id]: f })))
        .catch((e) => setByAccount((prev) => ({ ...prev, [a.id]: errorMessage(e) })));
    }
  }, [accounts]);

  if (accounts.length === 0) return null;

  return (
    <section>
      <h2 className="text-base font-semibold">Mail filters</h2>
      <p className="mt-1 text-sm text-gray-600">
        Rules your provider applies to incoming mail. Edit them in Gmail's settings; they apply
        before messages reach this app.
      </p>
      {accounts.map((a) => (
        <AccountFilters key={a.id} account={a} filters={byAccount[a.id]} />
      ))}
    </section>
  );
}

function AccountFilters({ account, filters }: { account: Account; filters: MailFilter[] | string | undefined }) {
  return (
    <div className="mt-3">
      <div className="text-sm font-medium">{account.email}</div>
      {filters === undefined && <div className="text-xs text-gray-500">Loading…</div>}
      {typeof filters === "string" && <div className="text-xs text-red-700">{filters}</div>}
      {Array.isArray(filters) && filters.length === 0 && (
        <div className="text-xs text-gray-500">No filters.</div>
      )}
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
              <span>
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
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
