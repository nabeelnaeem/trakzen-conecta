import { useEffect } from "react";
import { useMail } from "./store";
import { MessageList } from "./MessageList";
import { MessageView } from "./MessageView";
import { Composer } from "./Composer";
import type { Folder } from "../../lib/types";

const FOLDERS: { key: Folder; label: string }[] = [
  { key: "inbox", label: "Inbox" },
  { key: "starred", label: "Starred" },
  { key: "sent", label: "Sent" },
  { key: "drafts", label: "Drafts" },
  { key: "archive", label: "Archive" },
  { key: "trash", label: "Trash" },
  { key: "all", label: "All mail" },
];

export function MailView() {
  const s = useMail();

  useEffect(() => {
    void s.init();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (s.accounts.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 text-center">
        <div>
          <h2 className="text-lg font-semibold">No mail accounts yet</h2>
          <p className="mt-1 max-w-md text-gray-600">
            Add your Google OAuth client ID in Settings first, then connect a Gmail account.
            Sign-in happens in your browser; only a refresh token is kept, in the OS credential
            store.
          </p>
        </div>
        <button className="btn btn-primary" onClick={() => void s.addAccount()} disabled={s.busy}>
          {s.busy ? "Waiting for browser…" : "Connect Gmail"}
        </button>
        {s.error && <ErrorBanner />}
      </div>
    );
  }

  const active = s.accounts.find((a) => a.id === s.activeAccountId);
  const sync = s.activeAccountId !== null ? s.syncing[s.activeAccountId] : null;

  return (
    <div className="flex h-full">
      <aside className="flex w-52 shrink-0 flex-col border-r border-gray-200 bg-gray-50">
        <div className="p-3">
          <button className="btn btn-primary w-full justify-center" onClick={() => void s.openCompose()}>
            Compose
          </button>
        </div>
        <nav className="flex-1 px-2">
          {FOLDERS.map((f) => (
            <button
              key={f.key}
              onClick={() => s.setFolder(f.key)}
              className={`flex w-full items-center justify-between rounded-md px-3 py-1.5 text-left text-sm ${
                s.folder === f.key && !s.search
                  ? "bg-blue-100 font-medium text-blue-900"
                  : "hover:bg-gray-200"
              }`}
            >
              <span>{f.label}</span>
              {f.key === "inbox" && s.unread > 0 && (
                <span className="rounded-full bg-blue-600 px-1.5 text-xs text-white">{s.unread}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="border-t border-gray-200 p-2">
          <select
            className="input"
            value={s.activeAccountId ?? ""}
            onChange={(e) => s.setAccount(Number(e.target.value))}
          >
            {s.accounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.email}
              </option>
            ))}
          </select>
          <div className="mt-2 flex gap-1">
            <button className="btn btn-ghost flex-1 justify-center text-xs" onClick={() => void s.addAccount()} disabled={s.busy}>
              Add account
            </button>
            {active && (
              <button
                className="btn btn-ghost text-xs text-red-700"
                title={`Remove ${active.email}`}
                onClick={() => {
                  if (confirm(`Remove ${active.email} from this app?`)) void s.removeAccount(active.id);
                }}
              >
                Remove
              </button>
            )}
          </div>
        </div>
      </aside>

      <section className="flex w-[380px] shrink-0 flex-col border-r border-gray-200 bg-white">
        <div className="flex items-center gap-2 border-b border-gray-200 p-2">
          <input
            className="input"
            placeholder="Search subject, sender, snippet…"
            value={s.search}
            onChange={(e) => s.setSearch(e.target.value)}
          />
          <button
            className="btn btn-ghost"
            title="Sync now"
            onClick={() => void s.sync()}
            disabled={!!sync}
          >
            <span className={sync ? "animate-spin inline-block" : ""}>⟳</span>
          </button>
        </div>
        {sync && (
          <div className="border-b border-blue-100 bg-blue-50 px-3 py-1 text-xs text-blue-800">
            Syncing{sync.total > 0 ? ` ${sync.done}/${sync.total}` : "…"}
          </div>
        )}
        <MessageList />
      </section>

      <section className="flex min-w-0 flex-1 flex-col bg-white">
        {s.error && <ErrorBanner />}
        <MessageView />
      </section>

      {s.composer && <Composer />}
    </div>
  );
}

function ErrorBanner() {
  const { error, clearError } = useMail();
  return (
    <div className="flex items-start justify-between gap-3 border-b border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800">
      <span className="break-all">{error}</span>
      <button className="text-red-600 hover:text-red-900" onClick={clearError} aria-label="Dismiss">
        ✕
      </button>
    </div>
  );
}
