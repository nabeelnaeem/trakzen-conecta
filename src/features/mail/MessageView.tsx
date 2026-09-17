import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useMail } from "./store";
import { buildFrameDoc } from "./frame";
import { LabelChip } from "./LabelChip";
import { bytes, longDate } from "../../lib/format";
import { errorMessage, mail } from "../../lib/ipc";

export function MessageView() {
  const {
    detail,
    loadingDetail,
    selectedId,
    openCompose,
    toggleStar,
    trash,
    archive,
    markUnread,
    showImages: showImagesDefault,
    labels,
    modifyLabels,
  } = useMail();
  const [showImagesOnce, setShowImagesOnce] = useState(false);
  const [attError, setAttError] = useState<string | null>(null);
  const [labelMenu, setLabelMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  // Reset the per-message image choice whenever a different message opens.
  useEffect(() => {
    setShowImagesOnce(false);
    setAttError(null);
    setLabelMenu(false);
  }, [selectedId]);

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const data = e.data as { type?: string; href?: string } | null;
      if (data?.type !== "tc-open" || typeof data.href !== "string") return;
      if (/^(https?:|mailto:)/i.test(data.href)) void openUrl(data.href);
    };
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setLabelMenu(false);
    };
    window.addEventListener("message", onMessage);
    document.addEventListener("mousedown", onClick);
    return () => {
      window.removeEventListener("message", onMessage);
      document.removeEventListener("mousedown", onClick);
    };
  }, []);

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

  if (selectedId === null) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-gray-400">
        Select a message to read it
      </div>
    );
  }
  if (!detail || detail.id !== selectedId) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-gray-500">
        {loadingDetail ? "Loading…" : ""}
      </div>
    );
  }

  const userLabels = labels.filter((l) => l.kind === "user");
  const applied = detail.labels.filter((id) => userLabels.some((l) => l.remoteId === id));

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-1 border-b border-gray-200 px-3 py-2">
        <button className="btn" onClick={() => void openCompose("reply", detail.id)}>
          Reply
        </button>
        <button className="btn" onClick={() => void openCompose("reply-all", detail.id)}>
          Reply all
        </button>
        <button className="btn" onClick={() => void openCompose("forward", detail.id)}>
          Forward
        </button>
        <div className="flex-1" />
        <div className="relative" ref={menuRef}>
          <button className="btn btn-ghost" onClick={() => setLabelMenu((v) => !v)} title="Labels">
            Label ▾
          </button>
          {labelMenu && (
            <div className="absolute right-0 z-10 mt-1 max-h-72 w-56 overflow-y-auto rounded-md border border-gray-200 bg-white py-1 shadow-lg">
              {userLabels.length === 0 && (
                <div className="px-3 py-2 text-xs text-gray-500">No labels in this account</div>
              )}
              {userLabels.map((l) => {
                const on = detail.labels.includes(l.remoteId);
                return (
                  <label key={l.id} className="flex cursor-pointer items-center gap-2 px-3 py-1 text-sm hover:bg-gray-50">
                    <input
                      type="checkbox"
                      checked={on}
                      onChange={() =>
                        void modifyLabels(detail.id, on ? [] : [l.remoteId], on ? [l.remoteId] : [])
                      }
                    />
                    <span className="h-2.5 w-2.5 rounded-sm" style={{ background: l.bgColor ?? "#9ca3af" }} />
                    <span className="truncate">{l.name}</span>
                  </label>
                );
              })}
            </div>
          )}
        </div>
        <button
          className={`btn btn-ghost ${detail.isStarred ? "text-amber-500" : ""}`}
          onClick={() => void toggleStar(detail)}
          title={detail.isStarred ? "Unstar" : "Star"}
        >
          ★
        </button>
        <button className="btn btn-ghost" onClick={() => void markUnread(detail.id)} title="Mark as unread">
          Unread
        </button>
        {detail.labels.includes("INBOX") && (
          <button className="btn btn-ghost" onClick={() => void archive(detail.id)} title="Archive">
            Archive
          </button>
        )}
        <button className="btn btn-ghost text-red-700" onClick={() => void trash(detail.id)} title="Move to trash">
          Trash
        </button>
      </div>

      <div className="border-b border-gray-200 px-4 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <h1 className="text-lg font-semibold leading-snug">{detail.subject || "(no subject)"}</h1>
          {applied.map((id) => (
            <LabelChip key={id} remoteId={id} onRemove={() => void modifyLabels(detail.id, [], [id])} />
          ))}
        </div>
        <div className="mt-1 flex flex-wrap items-baseline gap-x-2 text-sm">
          <span className="font-medium">{detail.fromName}</span>
          <span className="text-gray-500">&lt;{detail.fromAddr}&gt;</span>
          <span className="ml-auto text-gray-500">{longDate(detail.date)}</span>
        </div>
        <div className="text-xs text-gray-500">
          to {detail.toAddrs || "—"}
          {detail.ccAddrs && <span>, cc {detail.ccAddrs}</span>}
        </div>
        {hasRemoteImages && !showImages && (
          <div className="mt-2 flex items-center gap-2 rounded bg-amber-50 px-2 py-1 text-xs text-amber-900">
            Remote images are blocked.
            <button className="underline" onClick={() => setShowImagesOnce(true)}>
              Show images
            </button>
          </div>
        )}
      </div>

      <iframe
        title="Message body"
        className="min-h-0 flex-1 w-full border-0 bg-white"
        sandbox="allow-scripts"
        srcDoc={doc}
      />

      {detail.attachments.length > 0 && (
        <div className="border-t border-gray-200 px-3 py-2">
          <div className="mb-1 text-xs font-medium text-gray-600">
            {detail.attachments.length} attachment{detail.attachments.length > 1 ? "s" : ""}
          </div>
          <div className="flex flex-wrap gap-2">
            {detail.attachments.map((a) => (
              <div key={a.id} className="flex items-center gap-2 rounded-md border border-gray-200 bg-gray-50 px-2 py-1 text-xs">
                <span className="max-w-[220px] truncate" title={a.filename}>
                  {a.filename}
                </span>
                <span className="text-gray-500">{bytes(a.size)}</span>
                <button
                  className="text-blue-700 hover:underline"
                  onClick={() =>
                    mail.saveAttachment(a.id, true).catch((e) => setAttError(errorMessage(e)))
                  }
                >
                  Open
                </button>
                <button
                  className="text-blue-700 hover:underline"
                  onClick={() =>
                    mail.saveAttachment(a.id, false).catch((e) => setAttError(errorMessage(e)))
                  }
                >
                  Save
                </button>
              </div>
            ))}
          </div>
          {attError && <div className="mt-1 text-xs text-red-700">{attError}</div>}
        </div>
      )}
    </div>
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
