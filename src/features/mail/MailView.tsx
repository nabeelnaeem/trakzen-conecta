import { useEffect } from "react";
import { useMail } from "./store";
import { MessageList } from "./MessageList";
import { MessageView } from "./MessageView";
import { Composer } from "./Composer";
import { LabelChip } from "./LabelChip";
import type { Category, Folder } from "../../lib/types";

const FOLDERS: { key: Folder; label: string }[] = [
  { key: "inbox", label: "Inbox" },
  { key: "starred", label: "Starred" },
  { key: "sent", label: "Sent" },
  { key: "drafts", label: "Drafts" },
  { key: "archive", label: "Archive" },
  { key: "trash", label: "Trash" },
  { key: "all", label: "All mail" },
];

const CATEGORIES: { key: Category; label: string; labelId: string }[] = [
  { key: "primary", label: "Primary", labelId: "CATEGORY_PERSONAL" },
  { key: "social", label: "Social", labelId: "CATEGORY_SOCIAL" },
  { key: "promotions", label: "Promotions", labelId: "CATEGORY_PROMOTIONS" },
  { key: "updates", label: "Updates", labelId: "CATEGORY_UPDATES" },
  { key: "forums", label: "Forums", labelId: "CATEGORY_FORUMS" },
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
  const userLabels = s.labels.filter((l) => l.kind === "user");
  const categoryUnread = (labelId: string) => s.labels.find((l) => l.remoteId === labelId)?.unread ?? 0;
  const inboxActive = s.folder === "inbox" && !s.label && !s.search;

  return (
    <div className="flex h-full">
      <aside className="flex w-52 shrink-0 flex-col border-r border-gray-200 bg-gray-50">
        <div className="p-3">
          <button className="btn btn-primary w-full justify-center" onClick={() => void s.openCompose()}>
            Compose
          </button>
        </div>
        <nav className="min-h-0 flex-1 overflow-y-auto px-2">
          {FOLDERS.map((f) => (
            <button
              key={f.key}
              onClick={() => s.setFolder(f.key)}
              className={`flex w-full items-center justify-between rounded-md px-3 py-1.5 text-left text-sm ${
                s.folder === f.key && !s.label && !s.search
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

          {userLabels.length > 0 && (
            <>
              <div className="mt-4 mb-1 px-3 text-[11px] font-semibold uppercase tracking-wide text-gray-500">
                Labels
              </div>
              {userLabels.map((l) => (
                <button
                  key={l.id}
                  onClick={() => s.setLabel(l.remoteId)}
                  title={`${l.total} message${l.total === 1 ? "" : "s"}`}
                  className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm ${
                    s.label === l.remoteId && !s.search ? "bg-blue-100 font-medium text-blue-900" : "hover:bg-gray-200"
                  }`}
                >
                  <span
                    className="h-2.5 w-2.5 shrink-0 rounded-sm"
                    style={{ background: l.bgColor ?? "#9ca3af" }}
                  />
                  <span className="min-w-0 flex-1 truncate">{l.name}</span>
                  {l.unread > 0 && <span className="text-xs font-semibold text-gray-700">{l.unread}</span>}
                </button>
              ))}
            </>
          )}
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

      <section className="flex w-[400px] shrink-0 flex-col border-r border-gray-200 bg-white">
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
        {inboxActive && (
          <div className="flex border-b border-gray-200 text-xs">
            {CATEGORIES.map((c) => {
              const unread = categoryUnread(c.labelId);
              return (
                <button
                  key={c.key}
                  onClick={() => s.setCategory(c.key)}
                  className={`flex-1 border-b-2 px-1 py-2 ${
                    s.category === c.key
                      ? "border-blue-600 font-medium text-blue-800"
                      : "border-transparent text-gray-600 hover:bg-gray-50"
                  }`}
                >
                  {c.label}
                  {c.key !== "primary" && unread > 0 && (
                    <span className="ml-1 text-[10px] text-gray-500">{unread}</span>
                  )}
                </button>
              );
            })}
          </div>
        )}
        {s.label && (
          <div className="flex items-center gap-2 border-b border-gray-200 px-3 py-1.5 text-xs">
            <LabelChip remoteId={s.label} />
            <button className="ml-auto text-gray-500 hover:text-gray-900" onClick={() => s.setFolder("inbox")}>
              ✕
            </button>
          </div>
        )}
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
