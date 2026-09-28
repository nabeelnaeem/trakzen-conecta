import { useEffect, useState } from "react";
import { errorMessage, files } from "../../lib/ipc";
import { confirmDialog } from "../../lib/confirm";
import { Spinner } from "../../lib/Spinner";
import type { FilesServer } from "../../lib/types";
import { useFiles } from "./store";

/** Settings → Trakzen Files: find the server and connect this app to an account. */
export function FilesTab() {
  const status = useFiles((s) => s.status);
  const setStatus = useFiles((s) => s.setStatus);
  const refresh = useFiles((s) => s.refresh);
  const [found, setFound] = useState<FilesServer[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const search = async () => {
    setSearching(true);
    try {
      const list = await files.discover();
      setFound(list);
      if (list.length && !url) setUrl(list[0].url);
    } catch (e) {
      setErr(errorMessage(e));
    } finally {
      setSearching(false);
    }
  };

  useEffect(() => {
    void refresh(true);
  }, [refresh]);

  useEffect(() => {
    if (status && !status.connected && found === null) void search();
    if (status?.url && !url) setUrl(status.url);
  }, [status]);

  const connect = async (target: string) => {
    setBusy(true);
    setErr(null);
    try {
      setStatus(await files.connect(target));
    } catch (e) {
      setErr(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    const ok = await confirmDialog({
      title: "Disconnect Trakzen Files?",
      message: "Conecta will stop being able to save files there. You can connect again any time.",
      confirmLabel: "Disconnect",
      danger: true,
    });
    if (!ok) return;
    try {
      await files.disconnect();
      await refresh();
    } catch (e) {
      setErr(errorMessage(e));
    }
  };

  return (
    <div className="space-y-6 text-sm">
      <p className="text-gray-600">
        Trakzen Files is your household file server. Once connected, received chat files and email attachments have a{" "}
        <b>Save to Trakzen Files</b> option.
      </p>

      {err && <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-red-800">{err}</div>}

      {status?.connected ? (
        <div className="rounded-lg border border-gray-200 p-4">
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">Connected</div>
          <p>
            Signed in as <b>{status.username}</b> on <span className="font-mono text-xs">{status.url}</span>
          </p>
          <div className="mt-3 flex gap-2">
            <button className="btn text-sm" onClick={() => void files.open("/").catch((e) => setErr(errorMessage(e)))}>
              Open in browser
            </button>
            <button className="btn btn-danger text-sm" onClick={() => void disconnect()}>
              Disconnect
            </button>
          </div>
        </div>
      ) : (
        <>
          {status?.url && (
            <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-amber-900">
              Not connected{status.username ? ` (was ${status.username})` : ""}. The server may have disconnected this app; connect again below.
            </div>
          )}
          <div>
            <div className="mb-2 flex items-center justify-between">
              <div className="text-xs font-semibold uppercase tracking-wide text-gray-500">On this network</div>
              <button className="btn-ghost rounded px-2 py-1 text-xs" onClick={() => void search()} disabled={searching}>
                {searching ? "Searching…" : "Search again"}
              </button>
            </div>
            {searching && found === null ? (
              <Spinner />
            ) : found && found.length > 0 ? (
              <ul className="divide-y divide-gray-100 rounded-md border border-gray-200">
                {found.map((f) => (
                  <li key={f.url} className="flex items-center gap-3 px-3 py-2">
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-medium">{f.name}</div>
                      <div className="font-mono text-xs text-gray-500">{f.url}</div>
                    </div>
                    <button className="btn btn-primary text-sm" disabled={busy} onClick={() => void connect(f.url)}>
                      Connect
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-gray-500">No server found automatically. Enter its address below.</p>
            )}
          </div>

          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">Or enter the address</div>
            <div className="flex gap-2">
              <input
                className="input flex-1 font-mono"
                placeholder="192.168.1.24:54380"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && url.trim() && void connect(url)}
              />
              <button className="btn btn-primary text-sm" disabled={busy || !url.trim()} onClick={() => void connect(url)}>
                Connect
              </button>
            </div>
          </div>

          {busy && (
            <p className="flex items-center gap-2 text-gray-600">
              <Spinner /> Waiting for you to approve in the browser…
            </p>
          )}
          <p className="text-xs text-gray-500">
            Connecting opens Trakzen Files in your browser; sign in there if asked and choose <b>Allow</b>. You can disconnect this app from
            either side later (Trakzen Files → Settings → Connected apps).
          </p>
        </>
      )}
    </div>
  );
}
