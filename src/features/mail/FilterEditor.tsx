import { useState } from "react";
import { useMail } from "./store";
import type { NewFilter } from "../../lib/types";

export function FilterEditor() {
  const { filterEditor, closeFilterEditor, createFilter, labels } = useMail();
  const [f, setF] = useState<NewFilter>(filterEditor!);
  const [busy, setBusy] = useState(false);
  if (!filterEditor) return null;
  const userLabels = labels.filter((l) => l.kind === "user");
  const set = (patch: Partial<NewFilter>) => setF({ ...f, ...patch });
  const hasCondition = [f.from, f.to, f.subject, f.hasWords, f.notWords].some((v) => v.trim()) || f.hasAttachment;
  const hasAction = f.skipInbox || f.markRead || f.star || !!f.addLabel || f.delete || f.neverSpam || f.markImportant;

  return (
    <div className="fixed inset-0 z-30 flex items-center justify-center bg-black/30 p-4" onClick={closeFilterEditor}>
      <div className="w-[560px] max-w-full rounded-lg border border-gray-300 bg-white shadow-xl" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-gray-200 bg-gray-50 px-4 py-2">
          <span className="font-medium">Create filter</span>
          <button className="text-gray-500 hover:text-gray-900" onClick={closeFilterEditor} aria-label="Close">
            ✕
          </button>
        </div>

        <div className="grid grid-cols-[110px_1fr] items-center gap-x-3 gap-y-2 px-4 py-3 text-sm">
          <label className="text-gray-600">From</label>
          <input className="input" value={f.from} onChange={(e) => set({ from: e.target.value })} autoFocus />
          <label className="text-gray-600">To</label>
          <input className="input" value={f.to} onChange={(e) => set({ to: e.target.value })} />
          <label className="text-gray-600">Subject</label>
          <input className="input" value={f.subject} onChange={(e) => set({ subject: e.target.value })} />
          <label className="text-gray-600">Has the words</label>
          <input className="input" value={f.hasWords} onChange={(e) => set({ hasWords: e.target.value })} />
          <label className="text-gray-600">Doesn't have</label>
          <input className="input" value={f.notWords} onChange={(e) => set({ notWords: e.target.value })} />
          <span />
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={f.hasAttachment} onChange={(e) => set({ hasAttachment: e.target.checked })} />
            Has attachment
          </label>
        </div>

        <div className="border-t border-gray-200 px-4 py-3 text-sm">
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">When a message matches</div>
          <div className="grid grid-cols-2 gap-x-4 gap-y-1.5">
            <Check label="Skip the Inbox (archive it)" v={f.skipInbox} on={(v) => set({ skipInbox: v })} />
            <Check label="Mark as read" v={f.markRead} on={(v) => set({ markRead: v })} />
            <Check label="Star it" v={f.star} on={(v) => set({ star: v })} />
            <Check label="Mark as important" v={f.markImportant} on={(v) => set({ markImportant: v })} />
            <Check label="Never send to spam" v={f.neverSpam} on={(v) => set({ neverSpam: v })} />
            <Check label="Delete it" v={f.delete} on={(v) => set({ delete: v })} />
            <label className="col-span-2 flex items-center gap-2">
              <input type="checkbox" checked={!!f.addLabel} onChange={(e) => set({ addLabel: e.target.checked ? (userLabels[0]?.remoteId ?? null) : null })} />
              Apply the label
              <select className="input w-auto" disabled={!f.addLabel} value={f.addLabel ?? ""} onChange={(e) => set({ addLabel: e.target.value || null })}>
                {userLabels.map((l) => (
                  <option key={l.id} value={l.remoteId}>
                    {l.name}
                  </option>
                ))}
              </select>
            </label>
            <Check label="Also apply to matching messages already in the mailbox" v={f.applyToExisting} on={(v) => set({ applyToExisting: v })} />
          </div>
        </div>

        <div className="flex items-center gap-2 border-t border-gray-200 bg-gray-50 px-4 py-2">
          <button
            className="btn btn-primary"
            disabled={!hasCondition || !hasAction || busy}
            onClick={async () => {
              setBusy(true);
              const ok = await createFilter(f);
              setBusy(false);
              if (ok) closeFilterEditor();
            }}
          >
            {busy ? "Creating…" : "Create filter"}
          </button>
          {!hasCondition && <span className="text-xs text-gray-500">Add at least one condition.</span>}
          {hasCondition && !hasAction && <span className="text-xs text-gray-500">Pick at least one action.</span>}
          <div className="flex-1" />
          <button className="btn btn-ghost" onClick={closeFilterEditor}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

function Check({ label, v, on }: { label: string; v: boolean; on: (v: boolean) => void }) {
  return (
    <label className="flex items-center gap-2">
      <input type="checkbox" checked={v} onChange={(e) => on(e.target.checked)} />
      {label}
    </label>
  );
}
