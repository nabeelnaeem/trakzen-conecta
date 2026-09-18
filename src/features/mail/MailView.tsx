import { useEffect, useRef, useState } from "react";
import { useMail } from "./store";
import { MessageList } from "./MessageList";
import { ThreadView } from "./ThreadView";
import { Composer } from "./Composer";
import { FilterEditor } from "./FilterEditor";
import { LabelChip } from "./LabelChip";
import { useMailShortcuts } from "./useShortcuts";
import { LabelTree } from "./LabelTree";
import { Spinner } from "../../lib/Spinner";
import type { Category, Folder } from "../../lib/types";
import { Archive, ChevronDown, ChevronRight, Clock, FileText, Inbox, Mails, PenLine, RefreshCw, Search, Send, ShieldAlert, Star, Tag, Trash2, type LucideIcon } from "lucide-react";

const FOLDERS: { key: Folder; label: string; icon: LucideIcon }[] = [
  { key: "inbox", label: "Inbox", icon: Inbox },
  { key: "starred", label: "Starred", icon: Star },
  { key: "snoozed", label: "Snoozed", icon: Clock },
  { key: "sent", label: "Sent", icon: Send },
  { key: "drafts", label: "Drafts", icon: FileText },
  { key: "archive", label: "Archive", icon: Archive },
  { key: "spam", label: "Spam", icon: ShieldAlert },
  { key: "trash", label: "Trash", icon: Trash2 },
  { key: "all", label: "All mail", icon: Mails },
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
  const [labelsOpen, setLabelsOpen] = useState(() => localStorage.getItem("tc.labelsOpen") !== "0");

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

  const sync =
    s.activeAccountId === 0
      ? Object.values(s.syncing).find((v) => v) ?? null
      : s.activeAccountId !== null
        ? s.syncing[s.activeAccountId]
        : null;
  const userLabels = s.labels.filter((l) => l.kind === "user");
  const categoryUnread = (labelId: string) => s.labels.find((l) => l.remoteId === labelId)?.unread ?? 0;
  const inboxActive = s.folder === "inbox" && !s.label && !s.search;
  const allSelected = s.messages.length > 0 && s.selected.length === s.messages.length;

  return (
    <div className="flex h-full">
      <aside className="flex w-56 shrink-0 flex-col border-r border-gray-200 bg-gray-50">
        <div className="p-3">
          <button className="btn btn-primary w-full justify-center shadow-sm" onClick={() => void s.openCompose()} title="Compose (c)">
            <PenLine size={16} /> Compose
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
              <f.icon size={16} strokeWidth={1.75} className="shrink-0 opacity-80" aria-hidden />
              <span className="flex-1">{f.label}</span>
              {f.key === "inbox" && s.unread > 0 && (
                <span className="rounded-full bg-blue-600 px-1.5 text-xs text-on-accent">{s.unread}</span>
              )}
            </button>
          ))}

          {userLabels.length > 0 && (
            <>
              <button
                className="mt-4 mb-1 flex w-full items-center gap-1 px-3 text-[11px] font-semibold uppercase tracking-wide text-gray-500 hover:text-gray-800"
                onClick={() => {
                  setLabelsOpen((v) => {
                    localStorage.setItem("tc.labelsOpen", v ? "0" : "1");
                    return !v;
                  });
                }}
              >
                {labelsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                Labels
                {!labelsOpen && userLabels.reduce((n, l) => n + l.unread, 0) > 0 && (
                  <span className="ml-auto normal-case tracking-normal text-gray-700">{userLabels.reduce((n, l) => n + l.unread, 0)}</span>
                )}
              </button>
              {labelsOpen && <LabelTree labels={userLabels} active={s.search ? null : s.label} onPick={(id) => s.setLabel(id)} />}
            </>
          )}
        </nav>
        <div className="border-t border-gray-200 p-2">
          <select className="input" value={s.activeAccountId ?? ""} onChange={(e) => s.setAccount(Number(e.target.value))}>
            {s.accounts.length > 1 && <option value={0}>All accounts</option>}
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
            <Search size={14} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-gray-400" aria-hidden />
          </div>
          <button className="btn btn-ghost" title="Sync now" onClick={() => void s.sync()} disabled={!!sync}>
            {sync ? <Spinner className="text-blue-600" /> : <RefreshCw size={16} />}
          </button>
        </div>

        {s.selected.length > 0 ? (
          <div className="flex flex-wrap items-center gap-x-0.5 gap-y-1 border-b border-gray-200 bg-blue-50 px-2 py-1 text-xs">
            <SelectMenu allSelected={allSelected} />
            <span className="mr-2 whitespace-nowrap font-medium">{s.selected.length} selected</span>
            <BulkButton title="Archive (e)" onClick={() => void s.act("archive")}><Archive size={15} /></BulkButton>
            <BulkButton title="Trash (#)" onClick={() => void s.act("trash")}><Trash2 size={15} /></BulkButton>
            <BulkButton title="Report spam (!)" onClick={() => void s.act("spam")}><ShieldAlert size={15} /></BulkButton>
            <BulkButton title="Mark as read (Shift+I)" onClick={() => void s.act("read")}><MailOpenIcon /></BulkButton>
            <BulkButton title="Mark as unread (Shift+U)" onClick={() => void s.act("unread")}><MailDot /></BulkButton>
            <BulkLabelMenu />
            <div className="flex-1" />
            <BulkButton title="Clear selection (Esc)" onClick={() => s.selectAll(false)}>✕</BulkButton>
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
        {s.selected.length > 0 && allSelected && !s.search && s.folder !== "snoozed" && (
          <div className="border-b border-gray-200 bg-blue-50/60 px-3 py-1.5 text-center text-xs text-gray-700">
            {s.allInView ? (
              <>
                All <b>{s.viewCount?.toLocaleString() ?? "…"}</b> {s.conversations ? "conversations" : "messages"} in {viewName(s)} are selected.{" "}
                <button className="text-blue-700 hover:underline" onClick={() => s.selectAll(false)}>
                  Clear selection
                </button>
              </>
            ) : (
              <>
                All <b>{s.messages.length}</b> on this page are selected.{" "}
                {s.viewCount !== null && s.viewCount > s.messages.length && (
                  <button className="text-blue-700 hover:underline" onClick={() => void s.selectEntireView()}>
                    Select all {s.viewCount.toLocaleString()} {s.conversations ? "conversations" : "messages"} in {viewName(s)}
                  </button>
                )}
              </>
            )}
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

function MailOpenIcon() {
  return <Mails size={15} />;
}
function MailDot() {
  return <span className="inline-block h-2.5 w-2.5 rounded-full bg-blue-600" />;
}

function BulkButton({ children, title, onClick }: { children: React.ReactNode; title: string; onClick: () => void }) {
  const working = useMail((m) => m.working);
  return (
    <button className="rounded px-2 py-1 text-sm hover:bg-blue-100 disabled:opacity-40" title={title} onClick={onClick} disabled={!!working}>
      {children}
    </button>
  );
}

function viewName(s: ReturnType<typeof useMail.getState>): string {
  if (s.label) return s.labels.find((l) => l.remoteId === s.label)?.name ?? "this label";
  if (s.folder === "inbox") return s.category === "primary" ? "Primary" : s.category[0].toUpperCase() + s.category.slice(1);
  return s.folder === "all" ? "All mail" : s.folder[0].toUpperCase() + s.folder.slice(1);
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
      <button className="flex items-center gap-1 rounded px-2 py-1 text-xs hover:bg-blue-100" title="Labels" onClick={() => setOpen((v) => !v)}>
        <Tag size={14} /> ▾
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
  const { pendingSend, undoSend, notice, working } = useMail();
  const [left, setLeft] = useState(0);
  useEffect(() => {
    if (!pendingSend) return;
    const tick = () => setLeft(Math.max(0, Math.ceil((pendingSend.sendAt - Date.now()) / 1000)));
    tick();
    const t = window.setInterval(tick, 250);
    return () => window.clearInterval(t);
  }, [pendingSend]);
  if (!pendingSend && !notice && !working) return null;
  return (
    <div className="pointer-events-none fixed bottom-4 left-1/2 z-40 flex -translate-x-1/2 flex-col items-center gap-2">
      {working && (
        <div className="flex items-center gap-2 rounded-full bg-gray-900 px-4 py-2 text-sm text-on-accent shadow-lg">
          <Spinner size={14} className="text-blue-300" /> {working}
        </div>
      )}
      {pendingSend && (
        <div className="pointer-events-auto flex items-center gap-3 rounded-full bg-gray-900 px-4 py-2 text-sm text-on-accent shadow-lg">
          Sending in {left}s…
          <button className="font-semibold text-blue-300 hover:text-blue-200" onClick={undoSend}>
            Undo
          </button>
        </div>
      )}
      {!pendingSend && notice && !working && (
        <div className="rounded-full bg-gray-900 px-4 py-2 text-sm text-on-accent shadow-lg">{notice}</div>
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
