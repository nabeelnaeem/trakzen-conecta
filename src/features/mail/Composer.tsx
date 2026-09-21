import { useEffect, useState } from "react";
import { confirmDialog } from "../../lib/confirm";
import { open } from "@tauri-apps/plugin-dialog";
import { useMail } from "./store";
import { buildFrameDoc } from "./frame";
import { RecipientInput } from "./RecipientInput";
import { settings } from "../../lib/ipc";

type Template = { name: string; subject: string; body: string };

export function Composer() {
  const { composer, updateComposer, closeCompose, discardDraft, send, scheduleSend, busy } = useMail();
  const [showCc, setShowCc] = useState(false);
  const [later, setLater] = useState("");
  const [templates, setTemplates] = useState<Template[]>([]);
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
  if (!composer) return null;
  const c = composer;

  const pickFiles = async () => {
    const picked = await open({ multiple: true, title: "Attach files" });
    if (!picked) return;
    const list = Array.isArray(picked) ? picked : [picked];
    updateComposer({ files: [...c.files, ...list.filter((p) => !c.files.includes(p))] });
  };

  const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

  return (
    <div className="fixed inset-0 z-20 flex items-end justify-end bg-black/20 p-4" onClick={closeCompose}>
      <div
        className="flex h-[80vh] w-[720px] max-w-full flex-col overflow-hidden rounded-lg border border-gray-300 bg-white shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-gray-200 bg-gray-50 px-3 py-2">
          <span className="text-sm font-medium">{c.subject || "New message"}</span>
          <span className="ml-auto mr-3 text-xs text-gray-500">
            {c.saving ? "Saving…" : c.dirty ? "Unsaved changes" : c.savedAt ? "Draft saved" : ""}
          </span>
          <button className="text-gray-500 hover:text-gray-900" onClick={closeCompose} aria-label="Close">
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

        <textarea
          className="min-h-[160px] flex-1 resize-none px-3 py-2 text-sm outline-none"
          placeholder="Write your message…"
          value={c.body}
          onChange={(e) => updateComposer({ body: e.target.value })}
        />

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
              <span key={p} className="flex items-center gap-1 rounded bg-blue-50 px-2 py-0.5">
                {fileName(p)}
                <button
                  className="text-gray-500 hover:text-red-700"
                  onClick={() => updateComposer({ files: c.files.filter((f) => f !== p) })}
                  aria-label={`Remove ${fileName(p)}`}
                >
                  ✕
                </button>
              </span>
            ))}
          </div>
        )}

        <div className="flex items-center gap-2 border-t border-gray-200 bg-gray-50 px-3 py-2">
          <button className="btn btn-primary" onClick={() => void send()} disabled={busy || !c.to.trim()}>
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
                if (t) updateComposer({ subject: t.subject || c.subject, body: t.body });
                e.target.value = "";
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
              disabled={!c.to.trim()}
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
