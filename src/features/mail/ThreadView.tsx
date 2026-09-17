import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useMail } from "./store";
import { buildFrameDoc } from "./frame";
import { LabelChip } from "./LabelChip";
import { Avatar } from "./Avatar";
import { bytes, longDate, shortDate } from "../../lib/format";
import { errorMessage, mail } from "../../lib/ipc";
import { Spinner } from "../../lib/Spinner";
import type { MessageDetail, MessageSummary } from "../../lib/types";

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
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-gray-400">
        Select a message to read it
      </div>
    );
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
          Reply
        </button>
        <button className="btn" onClick={() => void s.openCompose("reply-all", latest.id)} title="Reply all (a)">
          Reply all
        </button>
        <button className="btn" onClick={() => void s.openCompose("forward", latest.id)} title="Forward (f)">
          Forward
        </button>
        <div className="flex-1" />

        {inInbox && !inTrash && (
          <button className="btn btn-ghost" onClick={() => void s.act("archive", ids)} title="Archive (e)">
            Archive
          </button>
        )}
        {inSpam ? (
          <button className="btn btn-ghost" onClick={() => void s.act("notSpam", ids)} title="Not spam">
            Not spam
          </button>
        ) : (
          !inTrash && (
            <button className="btn btn-ghost" onClick={() => void s.act("spam", ids)} title="Report spam (!)">
              Spam
            </button>
          )
        )}
        {inTrash || inSpam ? (
          <button className="btn btn-ghost" onClick={() => void s.act("inbox", ids)} title="Move to inbox">
            To inbox
          </button>
        ) : (
          <button className="btn btn-ghost text-red-700" onClick={() => void s.act("trash", ids)} title="Delete (#)">
            Trash
          </button>
        )}

        <div className="relative">
          <button className="btn btn-ghost" onClick={() => setSnoozeMenu((v) => !v)} title="Snooze (b)">
            ⏰
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
            Label ▾
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
          ★
        </button>

        <div className="relative">
          <button className="btn btn-ghost" onClick={() => setMoreMenu((v) => !v)} title="More">
            ⋯
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

  const doc = useMemo(() => {
    if (!detail) return "";
    const body = detail.bodyHtml ?? `<pre>${escapeHtml(detail.bodyText ?? "")}</pre>`;
    return buildFrameDoc(body, showImages);
  }, [detail, showImages]);
  const hasRemoteImages = useMemo(
    () => !!detail?.bodyHtml && /<img[^>]+src=["']?https?:/i.test(detail.bodyHtml),
    [detail],
  );

  return (
    <div className={`mx-3 my-2 rounded-lg border bg-white ${expanded ? "border-gray-200 shadow-sm" : "border-gray-100"}`}>
      <div className="flex cursor-pointer items-start gap-3 px-4 py-3" onClick={onToggle}>
        <Avatar name={m.fromName || m.fromAddr} seed={m.fromAddr} size={32} />
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
          <button
            className="btn btn-ghost text-xs"
            onClick={(e) => {
              e.stopPropagation();
              onReply();
            }}
          >
            Reply
          </button>
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
