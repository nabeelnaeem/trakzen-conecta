import { useState } from "react";
import { Download, X } from "lucide-react";
import { Spinner } from "./Spinner";
import { useUpdater } from "./updater";

/** Bottom-corner note about a new version: progress while it downloads, then "Restart to update". */
export function UpdateBanner() {
  const u = useUpdater();
  const [notesOpen, setNotesOpen] = useState(false);
  const shown = (u.phase === "available" || u.phase === "downloading" || u.phase === "ready" || u.phase === "installing") && !u.dismissed;
  if (!shown) return null;

  const pct = u.total ? Math.min(100, Math.round((u.downloaded / u.total) * 100)) : null;
  return (
    <div role="status" className="fixed right-4 bottom-4 z-30 w-80 rounded-lg border border-gray-200 bg-white p-3 text-sm shadow-lg">
      <div className="flex items-start gap-2">
        <Download size={16} className="mt-0.5 shrink-0 text-blue-600" aria-hidden />
        <div className="min-w-0 flex-1">
          {u.phase === "downloading" ? (
            <>
              <div className="text-gray-700">Downloading version {u.version}…</div>
              <div className="mt-1.5 h-1 overflow-hidden rounded bg-gray-200">
                <div className="h-full bg-blue-600 transition-[width]" style={{ width: `${pct ?? 0}%` }} />
              </div>
            </>
          ) : u.phase === "available" ? (
            <div>Version {u.version} is available.</div>
          ) : (
            <div>
              Version {u.version} is ready — restart to update.
            </div>
          )}
          {u.error && <div className="mt-1 text-xs text-red-700">{u.error}</div>}
          {u.notes && u.phase !== "downloading" && (
            <button className="mt-1 text-xs text-blue-700 hover:underline" onClick={() => setNotesOpen((v) => !v)}>
              {notesOpen ? "Hide what's new" : "What's new"}
            </button>
          )}
          {notesOpen && u.notes && (
            <div className="mt-1 max-h-40 overflow-y-auto whitespace-pre-wrap rounded bg-gray-50 p-2 text-xs text-gray-700">{u.notes}</div>
          )}
          {u.phase === "available" && (
            <div className="mt-2 flex gap-2">
              <button className="btn btn-primary text-xs" onClick={() => void u.download(true)}>
                Download
              </button>
            </div>
          )}
          {(u.phase === "ready" || u.phase === "installing") && (
            <div className="mt-2 flex gap-2">
              <button className="btn btn-primary text-xs" disabled={u.phase === "installing"} onClick={() => void u.install()}>
                {u.phase === "installing" ? (
                  <span className="flex items-center gap-1.5">
                    <Spinner size={12} /> Installing…
                  </span>
                ) : (
                  "Restart to update"
                )}
              </button>
              <button className="btn btn-ghost text-xs" disabled={u.phase === "installing"} onClick={u.dismiss}>
                Later
              </button>
            </div>
          )}
        </div>
        {u.phase !== "installing" && (
          <button className="shrink-0 text-gray-400 hover:text-gray-700" onClick={u.dismiss} aria-label="Hide" title="Hide">
            <X size={14} />
          </button>
        )}
      </div>
    </div>
  );
}
