import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useMail } from "./store";
import { buildFrameDoc } from "./frame";
import { LabelChip } from "./LabelChip";
import { SenderAvatar } from "./SenderAvatar";
import { Paperclip } from "lucide-react";
import { bytes, longDate, shortDate } from "../../lib/format";
import { errorMessage, mail } from "../../lib/ipc";
import { Spinner } from "../../lib/Spinner";
import type { MessageDetail, MessageSummary } from "../../lib/types";
import { isDark, onTheme, themePrefs } from "../../lib/theme";
import { Archive, Clock, Forward, MoreHorizontal, Reply, ReplyAll, ShieldAlert, Star, Tag, Trash2 } from "lucide-react";

export function ThreadView() {
  const s = useMail();
  const { openId, thread, expanded, details, loadingDetail, labels, folder } = s;
  const [labelMenu, setLabelMenu] = useState(false);
  const [snoozeMenu, setSnoozeMenu] = useState(false);
  const [moreMenu, setMoreMenu] = useState(false);
  const menusRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const data = e.data as { type?: string; href?: string } | null;
      if (data?.type !== "tc-open" || typeof data.href !== "string") return;
      if (/^(https?:|mailto:)/i.test(data.href)) void openUrl(data.href);
    };
    const onClick = (e: MouseEvent) => {
      if (menusRef.current && !menusRef.current.contains(e.target as Node)) {
        setLabelMenu(false);
        setSnoozeMenu(false);
        setMoreMenu(false);
      }
    };
    window.addEventListener("message", onMessage);
    document.addEventListener("mousedown", onClick);
    return () => {
      window.removeEventListener("message", onMessage);
      document.removeEventListener("mousedown", onClick);
    };
  }, []);

  useEffect(() => {
    setLabelMenu(false);
    setSnoozeMenu(false);
    setMoreMenu(false);
  }, [openId]);

  if (openId === null) {
    return <EmptyPane />;
  }
  if (thread.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center gap-2 text-sm text-gray-500">
        {loadingDetail && (
          <>
            <Spinner className="text-blue-600" /> Loading…
          </>
        )}
      </div>
    );
  }

  const latest = thread[thread.length - 1];
  const subject = thread[0].subject || "(no subject)";
  const threadAttachments = thread.flatMap((m) => (details[m.id]?.attachments ?? []).map((a) => ({ ...a, from: m.fromName || m.fromAddr, mid: m.id })));
  const withAttachmentsPending = thread.filter((m) => m.hasAttachments && !details[m.id]).length;
  const userLabels = labels.filter((l) => l.kind === "user");
  const threadLabels = Array.from(new Set(thread.flatMap((m) => m.labels)));
  const applied = threadLabels.filter((id) => userLabels.some((l) => l.remoteId === id));
  const inInbox = threadLabels.includes("INBOX");
  const inSpam = threadLabels.includes("SPAM");
  const inTrash = threadLabels.includes("TRASH");
  const ids = [openId];

  const snoozeOptions = (): [string, number][] => {
    const now = new Date();
    const at = (d: Date, h: number) => {
      const x = new Date(d);
      x.setHours(h, 0, 0, 0);
      return x.getTime();
    };
    const tomorrow = new Date(now);
    tomorrow.setDate(now.getDate() + 1);
    const nextWeek = new Date(now);
    nextWeek.setDate(now.getDate() + ((8 - now.getDay()) % 7 || 7));
    const weekend = new Date(now);
    weekend.setDate(now.getDate() + ((6 - now.getDay() + 7) % 7 || 7));
    const opts: [string, number][] = [];
    if (now.getHours() < 17) opts.push(["Later today (6 PM)", at(now, 18)]);
    opts.push(["Tomorrow (8 AM)", at(tomorrow, 8)]);
    opts.push(["This weekend (Sat 8 AM)", at(weekend, 8)]);
    opts.push(["Next week (Mon 8 AM)", at(nextWeek, 8)]);
    return opts;
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-1 border-b border-gray-200 px-3 py-2" ref={menusRef}>
        <button className="btn" onClick={() => void s.openCompose("reply", latest.id)} title="Reply (r)">
          <Reply size={15} /> Reply
        </button>
        <button className="btn" onClick={() => void s.openCompose("reply-all", latest.id)} title="Reply all (a)">
          <ReplyAll size={15} /> Reply all
        </button>
        <button className="btn" onClick={() => void s.openCompose("forward", latest.id)} title="Forward (f)">
          <Forward size={15} /> Forward
        </button>
        <div className="flex-1" />

        {inInbox && !inTrash && (
          <button className="btn btn-ghost" onClick={() => void s.act("archive", ids)} title="Archive (e)">
            <Archive size={15} /> Archive
          </button>
        )}
        {inSpam ? (
          <button className="btn btn-ghost" onClick={() => void s.act("notSpam", ids)} title="Not spam">
            Not spam
          </button>
        ) : (
          !inTrash && (
            <button className="btn btn-ghost" onClick={() => void s.act("spam", ids)} title="Report spam (!)">
              <ShieldAlert size={15} /> Spam
            </button>
          )
        )}
        {inTrash || inSpam ? (
          <button className="btn btn-ghost" onClick={() => void s.act("inbox", ids)} title="Move to inbox">
            To inbox
          </button>
        ) : (
          <button className="btn btn-ghost btn-danger" onClick={() => void s.act("trash", ids)} title="Delete (#)">
            <Trash2 size={15} /> Trash
          </button>
        )}

        <div className="relative">
          <button className="btn btn-ghost" onClick={() => setSnoozeMenu((v) => !v)} title="Snooze (b)">
            <Clock size={15} />
          </button>
          {snoozeMenu && (
            <Menu>
              {folder === "snoozed" ? (
                <MenuItem onClick={() => void s.snooze(ids, null)}>Unsnooze</MenuItem>
              ) : (
                <>
                  {snoozeOptions().map(([label, ts]) => (
                    <MenuItem key={label} onClick={() => void s.snooze(ids, ts)}>
                      <span className="flex-1">{label}</span>
                      <span className="text-xs text-gray-500">{shortDate(ts)}</span>
                    </MenuItem>
                  ))}
                  <PickDateTime onPick={(ts) => void s.snooze(ids, ts)} />
                </>
              )}
            </Menu>
          )}
        </div>

        <div className="relative">
          <button className="btn btn-ghost" onClick={() => setLabelMenu((v) => !v)} title="Labels (l)">
            <Tag size={15} /> ▾
          </button>
          {labelMenu && (
            <Menu>
              {userLabels.length === 0 && <div className="px-3 py-2 text-xs text-gray-500">No labels in this account</div>}
              {userLabels.map((l) => {
                const on = threadLabels.includes(l.remoteId);
                return (
                  <label key={l.id} className="flex cursor-pointer items-center gap-2 px-3 py-1 text-sm hover:bg-gray-50">
                    <input
                      type="checkbox"
                      checked={on}
                      onChange={() => void s.modifyLabels(ids, on ? [] : [l.remoteId], on ? [l.remoteId] : [])}
                    />
                    <span className="h-2.5 w-2.5 rounded-sm" style={{ background: l.bgColor ?? "#9ca3af" }} />
                    <span className="truncate">{l.name}</span>
                  </label>
                );
              })}
              <NewLabel />
            </Menu>
          )}
        </div>

        <button
          className={`btn btn-ghost ${latest.isStarred ? "text-amber-500" : ""}`}
          onClick={() => void s.toggleStar(latest)}
          title="Star (s)"
        >
          <Star size={15} fill={latest.isStarred ? "currentColor" : "none"} />
        </button>

        <div className="relative">
          <button className="btn btn-ghost" onClick={() => setMoreMenu((v) => !v)} title="More">
            <MoreHorizontal size={15} />
          </button>
          {moreMenu && (
            <Menu>
              <MenuItem onClick={() => void s.act("unread", ids)}>Mark as unread</MenuItem>
              <MenuItem
                onClick={() => {
                  setMoreMenu(false);
                  s.openFilterEditor({ from: latest.fromAddr });
                }}
              >
                Filter messages like this
              </MenuItem>
            </Menu>
          )}
        </div>
      </div>

      <div className="border-b border-gray-200 px-4 py-2">
        <div className="flex flex-wrap items-center gap-2">
          <h1 className="text-lg font-semibold leading-snug">{subject}</h1>
          {applied.map((id) => (
            <LabelChip key={id} remoteId={id} onRemove={() => void s.modifyLabels(ids, [], [id])} />
          ))}
          {thread.length > 1 && <span className="text-xs text-gray-500">{thread.length} messages</span>}
        </div>
        {(threadAttachments.length > 0 || withAttachmentsPending > 0) && (
          <AttachmentsPanel
            items={threadAttachments}
            pending={withAttachmentsPending}
            onLoadAll={() => thread.filter((m) => m.hasAttachments && !details[m.id]).forEach((m) => void s.expand(m.id, true))}
          />
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto bg-gray-50">
        {thread.map((m) => (
          <ThreadMessage
            key={m.id}
            m={m}
            expanded={expanded.includes(m.id)}
            detail={details[m.id]}
            onToggle={() => void s.expand(m.id)}
            showImages={s.showImages}
            onReply={() => void s.openCompose("reply", m.id)}
          />
        ))}
      </div>
    </div>
  );
}

function ThreadMessage({
  m,
  expanded,
  detail,
  onToggle,
  showImages: showImagesDefault,
  onReply,
}: {
  m: MessageSummary;
  expanded: boolean;
  detail?: MessageDetail;
  onToggle: () => void;
  showImages: boolean;
  onReply: () => void;
}) {
  const [showImagesOnce, setShowImagesOnce] = useState(false);
  const [attError, setAttError] = useState<string | null>(null);
  const showImages = showImagesDefault || showImagesOnce;
  // Dark mode: invert per the appearance setting, with a per-message override.
  const [theme, setThemeState] = useState(themePrefs());
  useEffect(() => onTheme(setThemeState), []);
  const [invertOverride, setInvertOverride] = useState<boolean | null>(null);
  const darkNow = isDark(theme);
  const invert = darkNow && (invertOverride ?? theme.mailDark === "invert");

  const doc = useMemo(() => {
    if (!detail) return "";
    const body = detail.bodyHtml ?? `<pre>${escapeHtml(detail.bodyText ?? "")}</pre>`;
    return buildFrameDoc(body, showImages, invert);
  }, [detail, showImages, invert]);
  const hasRemoteImages = useMemo(
    () => !!detail?.bodyHtml && /<img[^>]+src=["']?https?:/i.test(detail.bodyHtml),
    [detail],
  );

  return (
    <div className={`mx-3 my-2 rounded-lg border bg-white ${expanded ? "border-gray-200 shadow-sm" : "border-gray-100"}`}>
      <div className="flex cursor-pointer items-start gap-3 px-4 py-3" onClick={onToggle}>
        <SenderAvatar name={m.fromName} email={m.fromAddr} size={32} />
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <span className={`truncate ${m.isRead ? "font-medium" : "font-semibold"}`}>{m.fromName || m.fromAddr}</span>
            {expanded && <span className="truncate text-xs text-gray-500">&lt;{m.fromAddr}&gt;</span>}
            <span className="ml-auto shrink-0 text-xs text-gray-500" title={longDate(m.date)}>
              {expanded ? longDate(m.date) : shortDate(m.date)}
            </span>
          </div>
          {expanded ? (
            <div className="text-xs text-gray-500">
              to {m.toAddrs || "—"}
              {m.ccAddrs && <span>, cc {m.ccAddrs}</span>}
            </div>
          ) : (
            <div className="truncate text-xs text-gray-500">{m.snippet}</div>
          )}
        </div>
        {expanded && (
          <span className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
            {detail?.canUnsubscribe && <UnsubscribeButton id={m.id} />}
            <button className="btn btn-ghost text-xs" onClick={onReply}>
              Reply
            </button>
          </span>
        )}
      </div>

      {expanded && (
        <>
          {hasRemoteImages && !showImages && (
            <div className="mx-4 mb-2 flex items-center gap-2 rounded bg-amber-50 px-2 py-1 text-xs text-amber-900">
              Remote images are blocked.
              <button className="underline" onClick={() => setShowImagesOnce(true)}>
                Show images
              </button>
            </div>
          )}
          {darkNow && detail && (
            <div className="mx-4 mb-1 text-right text-[11px] text-gray-500">
              <button className="hover:underline" onClick={() => setInvertOverride(!invert)}>
                {invert ? "Show original colours" : "Invert for dark mode"}
              </button>
            </div>
          )}
          {detail ? (
            <AutoHeightFrame doc={doc} />
          ) : (
            <div className="flex items-center gap-2 px-4 pb-4 text-sm text-gray-500">
              <Spinner size={14} className="text-blue-600" /> Loading…
            </div>
          )}
          {detail && detail.attachments.length > 0 && (
            <div className="border-t border-gray-100 px-4 py-2">
              <div className="flex flex-wrap gap-2">
                {detail.attachments.map((a) => (
                  <div key={a.id} className="flex items-center gap-2 rounded-md border border-gray-200 bg-gray-50 px-2 py-1 text-xs">
                    <span className="max-w-[220px] truncate" title={a.filename}>
                      {a.filename}
                    </span>
                    <span className="text-gray-500">{bytes(a.size)}</span>
                    <button className="text-blue-700 hover:underline" onClick={() => mail.saveAttachment(a.id, true).catch((e) => setAttError(errorMessage(e)))}>
                      Open
                    </button>
                    <button className="text-blue-700 hover:underline" onClick={() => mail.saveAttachment(a.id, false).catch((e) => setAttError(errorMessage(e)))}>
                      Save
                    </button>
                  </div>
                ))}
              </div>
              {attError && <div className="mt-1 text-xs text-red-700">{attError}</div>}
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** Sandboxed frame that grows to its content so a thread scrolls as one page. */
function AutoHeightFrame({ doc }: { doc: string }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(200);
  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const data = e.data as { type?: string; height?: number; id?: string } | null;
      if (data?.type === "tc-height" && e.source === ref.current?.contentWindow && typeof data.height === "number") {
        setHeight(Math.min(Math.max(80, data.height + 8), 20000));
      }
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);
  return <iframe ref={ref} title="Message body" className="w-full border-0 bg-white" style={{ height }} sandbox="allow-scripts" srcDoc={doc} />;
}

function EmptyPane() {
  const { unread, messages, folder, labels, label, conversations } = useMail();
  const where = label ? (labels.find((l) => l.remoteId === label)?.name ?? "this label") : folder === "all" ? "All mail" : folder[0].toUpperCase() + folder.slice(1);
  const unreadHere = messages.filter((m) => !m.isRead || m.threadUnread > 0).length;
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 px-8 text-center text-gray-500">
      <div className="text-3xl">📬</div>
      <div className="text-base text-gray-700">
        {unread === 0 ? "Inbox zero. Nothing unread." : `${unread} unread in your inbox`}
      </div>
      <div className="text-xs">
        {messages.length} {conversations ? "conversations" : "messages"} in {where}{unreadHere ? `, ${unreadHere} unread` : ""}
      </div>
      <div className="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-left text-xs">
        <kbd className="rounded border border-gray-300 bg-gray-50 px-1 font-mono">j / k</kbd><span>move through the list</span>
        <kbd className="rounded border border-gray-300 bg-gray-50 px-1 font-mono">e</kbd><span>archive</span>
        <kbd className="rounded border border-gray-300 bg-gray-50 px-1 font-mono">r</kbd><span>reply</span>
        <kbd className="rounded border border-gray-300 bg-gray-50 px-1 font-mono">c</kbd><span>compose</span>
        <kbd className="rounded border border-gray-300 bg-gray-50 px-1 font-mono">/</kbd><span>search (Enter searches Gmail)</span>
      </div>
    </div>
  );
}

function UnsubscribeButton({ id }: { id: number }) {
  const [state, setState] = useState<"idle" | "busy" | "done" | "error">("idle");
  const [note, setNote] = useState("");
  return (
    <button
      className="btn btn-ghost text-xs"
      disabled={state === "busy" || state === "done"}
      title="Uses the sender's List-Unsubscribe header"
      onClick={async () => {
        setState("busy");
        try {
          const r = await mail.unsubscribe(id);
          setState("done");
          setNote(r.method === "browser" ? "opened in browser" : r.method === "mailto" ? "request sent" : "done");
        } catch (e) {
          setState("error");
          setNote(errorMessage(e));
        }
      }}
    >
      {state === "busy" ? "Unsubscribing…" : state === "done" ? `Unsubscribed (${note})` : state === "error" ? `Unsubscribe failed: ${note}` : "Unsubscribe"}
    </button>
  );
}

function AttachmentsPanel({
  items,
  pending,
  onLoadAll,
}: {
  items: { id: number; filename: string; size: number; from: string; mid: number }[];
  pending: number;
  onLoadAll: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  return (
    <div className="mt-2 text-xs">
      <button className="flex items-center gap-1 text-gray-600 hover:text-gray-900" onClick={() => { setOpen((v) => !v); if (!open) onLoadAll(); }}>
        <Paperclip size={12} /> {items.length} attachment{items.length === 1 ? "" : "s"}
        {pending > 0 && ` (+ ${pending} message${pending === 1 ? "" : "s"} not loaded)`} {open ? "▾" : "▸"}
      </button>
      {open && (
        <ul className="mt-1 divide-y divide-gray-100 rounded-md border border-gray-200 bg-white">
          {items.map((a) => (
            <li key={a.id} className="flex items-center gap-2 px-2 py-1">
              <span className="min-w-0 flex-1 truncate" title={a.filename}>{a.filename}</span>
              <span className="text-gray-500">{bytes(a.size)}</span>
              <span className="max-w-[120px] truncate text-gray-400">{a.from}</span>
              <button className="text-blue-700 hover:underline" onClick={() => mail.saveAttachment(a.id, true).catch((e) => setErr(errorMessage(e)))}>Open</button>
              <button className="text-blue-700 hover:underline" onClick={() => mail.saveAttachment(a.id, false).catch((e) => setErr(errorMessage(e)))}>Save</button>
            </li>
          ))}
          {items.length === 0 && <li className="px-2 py-1 text-gray-500">Loading…</li>}
        </ul>
      )}
      {err && <div className="mt-1 text-red-700">{err}</div>}
    </div>
  );
}

function Menu({ children }: { children: React.ReactNode }) {
  return (
    <div className="absolute right-0 z-10 mt-1 max-h-80 w-64 overflow-y-auto rounded-md border border-gray-200 bg-white py-1 shadow-lg">
      {children}
    </div>
  );
}

function MenuItem({ children, onClick }: { children: React.ReactNode; onClick: () => void }) {
  return (
    <button className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-gray-50" onClick={onClick}>
      {children}
    </button>
  );
}

function PickDateTime({ onPick }: { onPick: (ts: number) => void }) {
  const [v, setV] = useState("");
  return (
    <div className="flex items-center gap-1 border-t border-gray-100 px-3 py-1.5">
      <input type="datetime-local" className="input text-xs" value={v} onChange={(e) => setV(e.target.value)} />
      <button
        className="btn btn-primary text-xs"
        disabled={!v}
        onClick={() => {
          const ts = new Date(v).getTime();
          if (!Number.isNaN(ts)) onPick(ts);
        }}
      >
        Set
      </button>
    </div>
  );
}

function NewLabel() {
  const { activeAccountId, loadLabels } = useMail();
  const [name, setName] = useState("");
  const [err, setErr] = useState<string | null>(null);
  return (
    <div className="border-t border-gray-100 px-3 py-1.5">
      <div className="flex gap-1">
        <input className="input text-xs" placeholder="New label" value={name} onChange={(e) => setName(e.target.value)} />
        <button
          className="btn text-xs"
          disabled={!name.trim() || activeAccountId === null}
          onClick={() =>
            mail
              .createLabel(activeAccountId!, name)
              .then(() => {
                setName("");
                void loadLabels();
              })
              .catch((e) => setErr(errorMessage(e)))
          }
        >
          Add
        </button>
      </div>
      {err && <div className="mt-1 text-xs text-red-700">{err}</div>}
    </div>
  );
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
