import { useEffect, useState } from "react";
import { confirmDialog } from "../../lib/confirm";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useMail } from "./store";
import { buildFrameDoc } from "./frame";
import { RecipientInput } from "./RecipientInput";
import { RichEditor } from "./RichEditor";
import { textToHtml } from "./store";
import { mail, settings } from "../../lib/ipc";
import { activeTab } from "../../lib/activeTab";
import { bytes } from "../../lib/format";

type Template = { name: string; subject: string; body: string };

const MB = 1024 * 1024;
/** Gmail's cap on attachments per message; most other providers are similar. */
const SIZE_LIMIT = 25 * MB;
const SIZE_WARN = 20 * MB;

export function Composer() {
  const { composer, updateComposer, closeCompose, discardDraft, send, scheduleSend, busy } = useMail();
  const [showCc, setShowCc] = useState(false);
  const [later, setLater] = useState("");
  const [templates, setTemplates] = useState<Template[]>([]);
  const [sizes, setSizes] = useState<Record<string, number | null>>({});
  const [dragging, setDragging] = useState(false);
  const files = composer?.files;
  useEffect(() => {
    void settings.get().then((s) => {
      try {
        const parsed = JSON.parse(s.mailTemplates || "[]") as Template[];
        if (Array.isArray(parsed)) setTemplates(parsed.filter((t) => t && t.name));
      } catch {
        setTemplates([]);
      }
    });
  }, []);

  useEffect(() => {
    const missing = (files ?? []).filter((p) => !(p in sizes));
    if (missing.length === 0) return;
    mail
      .fileSizes(missing)
      .then((r) => setSizes((cur) => ({ ...cur, ...Object.fromEntries(missing.map((p, i) => [p, r[i]])) })))
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [files]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (activeTab() !== "mail") return;
        if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          const paths = e.payload.paths;
          // Folders come through too; only regular files can be attached.
          void mail.fileSizes(paths).then((r) => {
            const ok = paths.filter((_, i) => r[i] !== null);
            setSizes((cur) => ({ ...cur, ...Object.fromEntries(paths.map((p, i) => [p, r[i]])) }));
            const now = useMail.getState().composer;
            const fresh = ok.filter((p) => !now?.files.includes(p));
            if (now && fresh.length) updateComposer({ files: [...now.files, ...fresh] });
          });
        }
      })
      .then((u) => {
        if (gone) u();
        else unlisten = u;
      });
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [updateComposer]);

  if (!composer) return null;
  const c = composer;

  const pickFiles = async () => {
    const picked = await open({ multiple: true, title: "Attach files" });
    if (!picked) return;
    const list = Array.isArray(picked) ? picked : [picked];
    updateComposer({ files: [...c.files, ...list.filter((p) => !c.files.includes(p))] });
  };

  const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;
  const totalSize = c.files.reduce((n, p) => n + (sizes[p] ?? 0), 0);
  const hasRecipient = !!(c.to.trim() || c.cc.trim() || c.bcc.trim());

  const applyTemplate = async (t: Template) => {
    if (c.body.trim()) {
      const ok = await confirmDialog({
        title: `Replace your message with "${t.name}"?`,
        message: "What you have typed so far will be lost.",
        confirmLabel: "Replace",
        danger: true,
      });
      if (!ok) return;
    }
    const now = useMail.getState().composer ?? c;
    updateComposer({ subject: t.subject || now.subject, body: t.body, bodyHtml: textToHtml(t.body) });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.defaultPrevented) return;
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      if (hasRecipient && !busy) void send();
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeCompose();
    }
  };

  return (
    <div className="fixed inset-0 z-20 flex items-end justify-end bg-black/20 p-4">
      <div
        className="relative flex h-[80vh] w-[720px] max-w-full flex-col overflow-hidden rounded-lg border border-gray-300 bg-white shadow-xl"
        onKeyDown={onKeyDown}
      >
        {dragging && (
          <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-lg border-2 border-dashed border-blue-500 bg-blue-50/80 text-sm font-medium text-blue-800">
            Drop files to attach
          </div>
        )}
        <div className="flex items-center justify-between border-b border-gray-200 bg-gray-50 px-3 py-2">
          <span className="text-sm font-medium">{c.subject || "New message"}</span>
          <span className="ml-auto mr-3 text-xs text-gray-500">
            {c.saving ? "Saving…" : c.dirty ? "Unsaved changes" : c.savedAt ? "Draft saved" : ""}
          </span>
          <button className="text-gray-500 hover:text-gray-900" onClick={closeCompose} aria-label="Close" title="Close and keep draft (Esc)">
            ✕
          </button>
        </div>

        <div className="space-y-1 border-b border-gray-200 px-3 py-2 text-sm">
          <Row label="To">
            <RecipientInput
              accountId={c.accountId}
              autoFocus
              value={c.to}
              onChange={(to) => updateComposer({ to })}
              placeholder="name@example.com, other@example.com"
            />
            {!showCc && (
              <button className="text-xs text-gray-500 hover:text-gray-800" onClick={() => setShowCc(true)}>
                Cc/Bcc
              </button>
            )}
          </Row>
          {(showCc || c.cc || c.bcc) && (
            <>
              <Row label="Cc">
                <RecipientInput accountId={c.accountId} value={c.cc} onChange={(cc) => updateComposer({ cc })} />
              </Row>
              <Row label="Bcc">
                <RecipientInput accountId={c.accountId} value={c.bcc} onChange={(bcc) => updateComposer({ bcc })} />
              </Row>
            </>
          )}
          <Row label="Subject">
            <input className="flex-1 outline-none" value={c.subject} onChange={(e) => updateComposer({ subject: e.target.value })} />
          </Row>
        </div>

        <RichEditor html={c.bodyHtml} placeholder="Write your message…" onChange={(bodyHtml, body) => updateComposer({ bodyHtml, body })} />

        {c.draft?.quotedHtml && (
          <details className="border-t border-gray-200">
            <summary className="cursor-pointer px-3 py-1 text-xs text-gray-500">Quoted message</summary>
            <iframe
              title="Quoted message"
              className="h-40 w-full border-0"
              sandbox=""
              srcDoc={buildFrameDoc(c.draft.quotedHtml, false)}
            />
          </details>
        )}

        {(c.files.length > 0 || (c.draft?.attachmentNames.length ?? 0) > 0) && (
          <div className="flex flex-wrap gap-1 border-t border-gray-200 px-3 py-2 text-xs">
            {c.draft?.attachmentNames.map((n) => (
              <span key={`fwd-${n}`} className="rounded bg-gray-100 px-2 py-0.5" title="Forwarded attachment">
                {n}
              </span>
            ))}
            {c.files.map((p) => (
              <span key={p} className="flex items-center gap-1 rounded bg-blue-50 px-2 py-0.5" title={p}>
                {fileName(p)}
                {typeof sizes[p] === "number" && <span className="text-gray-500">{bytes(sizes[p])}</span>}
                <button
                  className="text-gray-500 hover:text-red-700"
                  onClick={() => updateComposer({ files: c.files.filter((f) => f !== p) })}
                  aria-label={`Remove ${fileName(p)}`}
                >
                  ✕
                </button>
              </span>
            ))}
            {c.files.length > 0 && (
              <span
                className={`ml-auto self-center ${totalSize > SIZE_LIMIT ? "font-medium text-red-700" : totalSize > SIZE_WARN ? "text-amber-700" : "text-gray-500"}`}
              >
                {totalSize > SIZE_LIMIT
                  ? `Total ${bytes(totalSize)}: over the 25 MB limit of Gmail and most providers`
                  : totalSize > SIZE_WARN
                    ? `Total ${bytes(totalSize)}: close to Gmail's 25 MB limit`
                    : `Total ${bytes(totalSize)}`}
              </span>
            )}
          </div>
        )}

        <div className="flex items-center gap-2 border-t border-gray-200 bg-gray-50 px-3 py-2">
          <button className="btn btn-primary" onClick={() => void send()} disabled={busy || !hasRecipient} title="Send (Ctrl+Enter)">
            {busy ? "Sending…" : "Send"}
          </button>
          <button className="btn" onClick={() => void pickFiles()}>
            Attach
          </button>
          {templates.length > 0 && (
            <select
              className="input w-40 text-xs"
              defaultValue=""
              onChange={(e) => {
                const t = templates.find((x) => x.name === e.target.value);
                e.target.value = "";
                if (t) void applyTemplate(t);
              }}
            >
              <option value="">Template…</option>
              {templates.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.name}
                </option>
              ))}
            </select>
          )}
          <input
            type="datetime-local"
            className="input w-auto text-xs"
            value={later}
            onChange={(e) => setLater(e.target.value)}
            title="Send later"
          />
          {later && (
            <button
              className="btn text-xs"
              disabled={!hasRecipient}
              onClick={() => void scheduleSend(new Date(later).getTime())}
            >
              Schedule
            </button>
          )}
          <div className="flex-1" />
          <button
            className="btn btn-ghost text-red-700"
            onClick={() => {
              if (!c.draftId) void discardDraft();
              else void confirmDialog({ title: "Discard this draft?", confirmLabel: "Discard", danger: true }).then((ok) => {
                  if (ok) void discardDraft();
                });
            }}
            title="Delete draft"
          >
            Discard
          </button>
        </div>
      </div>
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="relative flex items-center gap-2 border-b border-gray-100 py-1 last:border-0">
      <span className="w-14 shrink-0 text-gray-500">{label}</span>
      {children}
    </div>
  );
}
