import { useEffect, useRef } from "react";
import { create } from "zustand";

export interface ConfirmOptions {
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** Styles the confirm button as destructive. */
  danger?: boolean;
}

interface Pending extends ConfirmOptions {
  resolve: (ok: boolean) => void;
}

const useConfirm = create<{ pending: Pending | null; ask: (p: Pending) => void; answer: (ok: boolean) => void }>((set, get) => ({
  pending: null,
  ask: (p) => {
    get().pending?.resolve(false);
    set({ pending: p });
  },
  answer: (ok) => {
    get().pending?.resolve(ok);
    set({ pending: null });
  },
}));

/** In-app replacement for `window.confirm`; resolves to true when confirmed. */
export function confirmDialog(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => useConfirm.getState().ask({ ...opts, resolve }));
}

/** Mount once near the app root. */
export function ConfirmHost() {
  const pending = useConfirm((s) => s.pending);
  const answer = useConfirm((s) => s.answer);
  const okRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!pending) return;
    okRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") answer(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pending, answer]);
  if (!pending) return null;
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4" onClick={() => answer(false)}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        className="w-[380px] rounded-lg border border-gray-300 bg-white p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div id="confirm-title" className="font-medium">
          {pending.title}
        </div>
        {pending.message && <p className="mt-1 text-sm text-gray-600">{pending.message}</p>}
        <div className="mt-4 flex justify-end gap-2">
          <button className="btn text-sm" onClick={() => answer(false)}>
            {pending.cancelLabel ?? "Cancel"}
          </button>
          <button ref={okRef} className={`btn text-sm ${pending.danger ? "btn-danger" : "btn-primary"}`} onClick={() => answer(true)}>
            {pending.confirmLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
