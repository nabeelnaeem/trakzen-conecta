import { useEffect, useRef, useState } from "react";
import { useMail } from "./store";
import { MessageList } from "./MessageList";
import { ThreadView } from "./ThreadView";
import { Composer } from "./Composer";
import { FilterEditor } from "./FilterEditor";
import { LabelChip } from "./LabelChip";
import { useMailShortcuts } from "./useShortcuts";
import type { Category, Folder } from "../../lib/types";

const FOLDERS: { key: Folder; label: string; icon: string }[] = [
  { key: "inbox", label: "Inbox", icon: "📥" },
  { key: "starred", label: "Starred", icon: "★" },
  { key: "snoozed", label: "Snoozed", icon: "⏰" },
  { key: "sent", label: "Sent", icon: "📤" },
  { key: "drafts", label: "Drafts", icon: "📝" },
  { key: "archive", label: "Archive", icon: "🗄" },
  { key: "spam", label: "Spam", icon: "⚠" },
  { key: "trash", label: "Trash", icon: "🗑" },
  { key: "all", label: "All mail", icon: "✉" },
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
  const searchRef = useRef<HTMLInputElement>(null);
  useMailShortcuts(searchRef);

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
  const allSelected = s.messages.length > 0 && s.selected.length === s.messages.length;

  return (
    <div className="flex h-full">
      <aside className="flex w-56 shrink-0 flex-col border-r border-gray-200 bg-gray-50">
        <div className="p-3">
          <button className="btn btn-primary w-full justify-center shadow-sm" onClick={() => void s.openCompose()} title="Compose (c)">
            ✎ Compose
          </button>
        </div>
        <nav className="min-h-0 flex-1 overflow-y-auto px-2">
          {FOLDERS.map((f) => (
            <button
              key={f.key}
              onClick={() => s.setFolder(f.key)}
              className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm ${
                s.folder === f.key && !s.label && !s.search
                  ? "bg-blue-100 font-medium text-blue-900"
                  : "text-gray-700 hover:bg-gray-200"
              }`}
            >
              <span className="w-4 text-center text-xs opacity-70">{f.icon}</span>
              <span className="flex-1">{f.label}</span>
              {f.key === "inbox" && s.unread > 0 && (
                <span className="rounded-full bg-blue-600 px-1.5 text-xs text-white">{s.unread}</span>
              )}
            </button>
          ))}

          {userLabels.length > 0 && (
            <>
              <div className="mt-4 mb-1 flex items-center justify-between px-3 text-[11px] font-semibold uppercase tracking-wide text-gray-500">
                Labels
              </div>
              {userLabels.map((l) => (
                <button
                  key={l.id}
                  onClick={() => s.setLabel(l.remoteId)}
                  title={`${l.total} message${l.total === 1 ? "" : "s"}`}
                  className={`flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm ${
                    s.label === l.remoteId && !s.search ? "bg-blue-100 font-medium text-blue-900" : "text-gray-700 hover:bg-gray-200"
                  }`}
                >
                  <span className="h-2.5 w-2.5 shrink-0 rounded-sm" style={{ background: l.bgColor ?? "#9ca3af" }} />
                  <span className="min-w-0 flex-1 truncate">{l.name}</span>
                  {l.unread > 0 && <span className="text-xs font-semibold text-gray-700">{l.unread}</span>}
                </button>
              ))}
            </>
          )}
        </nav>
        <div className="border-t border-gray-200 p-2">
          <select className="input" value={s.activeAccountId ?? ""} onChange={(e) => s.setAccount(Number(e.target.value))}>
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

      <section className="flex w-[420px] shrink-0 flex-col border-r border-gray-200 bg-white">
        <div className="flex items-center gap-2 border-b border-gray-200 p-2">
          <div className="relative flex-1">
            <input
              ref={searchRef}
              className="input pl-7"
              placeholder="Search mail  (Enter searches Gmail)"
              value={s.search}
              onChange={(e) => s.setSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void s.searchOnServer();
                if (e.key === "Escape") s.setSearch("");
              }}
            />
            <span className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-gray-400">⌕</span>
          </div>
          <button className="btn btn-ghost" title="Sync now" onClick={() => void s.sync()} disabled={!!sync}>
            <span className={sync ? "animate-spin inline-block" : ""}>⟳</span>
          </button>
        </div>

        {s.selected.length > 0 ? (
          <div className="flex items-center gap-1 border-b border-gray-200 bg-blue-50 px-2 py-1 text-xs">
            <SelectMenu allSelected={allSelected} />
            <span className="mr-1 font-medium">{s.selected.length} selected</span>
            <button className="btn btn-ghost text-xs" onClick={() => void s.act("archive")}>Archive</button>
            <button className="btn btn-ghost text-xs" onClick={() => void s.act("trash")}>Trash</button>
            <button className="btn btn-ghost text-xs" onClick={() => void s.act("spam")}>Spam</button>
            <button className="btn btn-ghost text-xs" onClick={() => void s.act("read")}>Read</button>
            <button className="btn btn-ghost text-xs" onClick={() => void s.act("unread")}>Unread</button>
            <BulkLabelMenu />
            <button className="btn btn-ghost text-xs" onClick={() => s.selectAll(false)} title="Clear (Esc)">✕</button>
          </div>
        ) : inboxActive ? (
          <div className="flex border-b border-gray-200 text-xs">
            {CATEGORIES.map((c) => {
              const unread = categoryUnread(c.labelId);
              return (
                <button
                  key={c.key}
                  onClick={() => s.setCategory(c.key)}
                  className={`flex-1 border-b-2 px-1 py-2 ${
                    s.category === c.key ? "border-blue-600 font-medium text-blue-800" : "border-transparent text-gray-600 hover:bg-gray-50"
                  }`}
                >
                  {c.label}
                  {c.key !== "primary" && unread > 0 && <span className="ml-1 text-[10px] text-gray-500">{unread}</span>}
                </button>
              );
            })}
          </div>
        ) : null}

        {s.selected.length === 0 && s.messages.length > 0 && !s.search && (
          <div className="flex items-center gap-2 border-b border-gray-100 px-2 py-1 text-xs text-gray-500">
            <SelectMenu allSelected={false} />
            <span>{s.messages.length}{s.hasMore ? "+" : ""} {s.conversations ? "conversations" : "messages"}</span>
            <div className="flex-1" />
            <button className="hover:text-gray-900 hover:underline" onClick={() => void s.markView(true)} title="Mark everything in this view as read">
              Mark all as read
            </button>
            <button className="hover:text-gray-900 hover:underline" onClick={() => void s.markView(false)} title="Mark everything in this view as unread">
              Mark all as unread
            </button>
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
        {s.serverSearch && (
          <div className="flex items-center gap-2 border-b border-gray-200 bg-gray-50 px-3 py-1 text-xs text-gray-600">
            Results from Gmail
            <button className="ml-auto underline" onClick={() => s.setSearch(s.search)}>
              back to local
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
        <ThreadView />
      </section>

      {s.composer && <Composer />}
      {s.filterEditor && <FilterEditor />}
      <Toasts />
    </div>
  );
}

/** Gmail's select checkbox with its All / None / Read / Unread menu. */
function SelectMenu({ allSelected }: { allSelected: boolean }) {
  const { selectAll, selectWhere, markView } = useMail();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);
  const item = (label: string, fn: () => void) => (
    <button
      className="block w-full px-3 py-1.5 text-left text-sm hover:bg-gray-50"
      onClick={() => {
        fn();
        setOpen(false);
      }}
    >
      {label}
    </button>
  );
  return (
    <div className="relative flex items-center" ref={ref}>
      <input type="checkbox" checked={allSelected} onChange={(e) => selectAll(e.target.checked)} title="Select all (*)" />
      <button className="px-1 text-gray-500 hover:text-gray-900" onClick={() => setOpen((v) => !v)} aria-label="Select options">
        ▾
      </button>
      {open && (
        <div className="absolute left-0 top-full z-10 mt-1 w-48 rounded-md border border-gray-200 bg-white py-1 shadow-lg">
          {item("All", () => selectAll(true))}
          {item("None", () => selectAll(false))}
          {item("Read", () => selectWhere("read"))}
          {item("Unread", () => selectWhere("unread"))}
          <div className="my-1 border-t border-gray-100" />
          {item("Mark all as read", () => void markView(true))}
          {item("Mark all as unread", () => void markView(false))}
        </div>
      )}
    </div>
  );
}

function BulkLabelMenu() {
  const { labels, modifyLabels, selected } = useMail();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);
  const userLabels = labels.filter((l) => l.kind === "user");
  return (
    <div className="relative" ref={ref}>
      <button className="btn btn-ghost text-xs" onClick={() => setOpen((v) => !v)}>
        Label ▾
      </button>
      {open && (
        <div className="absolute left-0 z-10 mt-1 max-h-72 w-56 overflow-y-auto rounded-md border border-gray-200 bg-white py-1 shadow-lg">
          {userLabels.map((l) => (
            <div key={l.id} className="flex items-center gap-1 px-2 py-1 text-sm">
              <span className="h-2.5 w-2.5 rounded-sm" style={{ background: l.bgColor ?? "#9ca3af" }} />
              <span className="min-w-0 flex-1 truncate">{l.name}</span>
              <button className="text-xs text-green-700 hover:underline" onClick={() => void modifyLabels(selected, [l.remoteId], [])}>
                add
              </button>
              <button className="text-xs text-red-700 hover:underline" onClick={() => void modifyLabels(selected, [], [l.remoteId])}>
                remove
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Toasts() {
  const { pendingSend, undoSend, notice } = useMail();
  const [left, setLeft] = useState(0);
  useEffect(() => {
    if (!pendingSend) return;
    const tick = () => setLeft(Math.max(0, Math.ceil((pendingSend.sendAt - Date.now()) / 1000)));
    tick();
    const t = window.setInterval(tick, 250);
    return () => window.clearInterval(t);
  }, [pendingSend]);
  if (!pendingSend && !notice) return null;
  return (
    <div className="pointer-events-none fixed bottom-4 left-1/2 z-40 -translate-x-1/2">
      {pendingSend && (
        <div className="pointer-events-auto flex items-center gap-3 rounded-full bg-gray-900 px-4 py-2 text-sm text-white shadow-lg">
          Sending in {left}s…
          <button className="font-semibold text-blue-300 hover:text-blue-200" onClick={undoSend}>
            Undo
          </button>
        </div>
      )}
      {!pendingSend && notice && (
        <div className="rounded-full bg-gray-900 px-4 py-2 text-sm text-white shadow-lg">{notice}</div>
      )}
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
