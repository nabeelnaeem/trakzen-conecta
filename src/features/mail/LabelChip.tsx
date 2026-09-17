import { useMail } from "./store";

/** Small coloured pill for a user label, resolved from the label cache. */
export function LabelChip({ remoteId, onRemove }: { remoteId: string; onRemove?: () => void }) {
  const label = useMail((s) => s.labels.find((l) => l.remoteId === remoteId));
  if (!label || label.kind !== "user") return null;
  return (
    <span
      className="inline-flex max-w-[160px] items-center gap-1 rounded px-1.5 py-0.5 text-[11px] leading-4"
      style={{ background: label.bgColor ?? "#e5e7eb", color: label.fgColor ?? "#111827" }}
      title={label.name}
    >
      <span className="truncate">{label.name}</span>
      {onRemove && (
        <button className="opacity-70 hover:opacity-100" onClick={onRemove} aria-label={`Remove ${label.name}`}>
          ✕
        </button>
      )}
    </span>
  );
}
