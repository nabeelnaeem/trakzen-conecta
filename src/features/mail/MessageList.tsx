import { useMail } from "./store";
import { LabelChip } from "./LabelChip";
import { Avatar } from "./Avatar";
import { shortDate } from "../../lib/format";
import { Spinner } from "../../lib/Spinner";
import type { SnoozedMessage } from "../../lib/types";

export function MessageList() {
  const {
    messages,
    openId,
    open,
    toggleStar,
    search,
    folder,
    label,
    labels,
    fetching,
    hasMore,
    loadMore,
    selected,
    toggleSelect,
    serverSearch,
  } = useMail();

  const userLabelIds = new Set(labels.filter((l) => l.kind === "user").map((l) => l.remoteId));
  const selectedSet = new Set(selected);

  if (messages.length === 0) {
    const where = label ? (labels.find((l) => l.remoteId === label)?.name ?? "this label") : folder;
    return (
      <div className="flex flex-1 items-center justify-center gap-2 p-6 text-center text-sm text-gray-500">
        {fetching ? (
          <>
            <Spinner className="text-blue-600" /> Loading…
          </>
        ) : search ? (
          "No messages match."
        ) : (
          `Nothing in ${where}.`
        )}
      </div>
    );
  }

  return (
    <ul className="flex-1 overflow-y-auto">
      {messages.map((m) => {
        const isOpen = m.id === openId;
        const isSel = selectedSet.has(m.id);
        const chips = m.labels.filter((l) => userLabelIds.has(l) && l !== label).slice(0, 2);
        const unread = !m.isRead || m.threadUnread > 0;
        const until = (m as SnoozedMessage).until;
        return (
          <li
            key={m.id}
            onClick={(e) => {
              if (e.shiftKey) {
                toggleSelect(m.id, true);
                return;
              }
              void open(m.id);
            }}
            className={`group flex cursor-pointer gap-2 border-b border-gray-100 px-2 py-2 ${
              isOpen ? "bg-blue-50" : isSel ? "bg-blue-50/60" : unread ? "bg-white hover:bg-gray-50" : "bg-gray-50/40 hover:bg-gray-50"
            }`}
          >
            <div className="flex shrink-0 flex-col items-center gap-1 pt-0.5">
              <input
                type="checkbox"
                className={`h-3.5 w-3.5 cursor-pointer ${isSel || selected.length ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}
                checked={isSel}
                onClick={(e) => e.stopPropagation()}
                onChange={() => toggleSelect(m.id)}
                aria-label="Select"
              />
              <button
                className={`text-base leading-none ${m.isStarred ? "text-amber-500" : "text-gray-300 hover:text-gray-500"}`}
                onClick={(e) => {
                  e.stopPropagation();
                  void toggleStar(m);
                }}
                aria-label={m.isStarred ? "Unstar" : "Star"}
              >
                ★
              </button>
            </div>
            <Avatar name={m.fromName || m.fromAddr} seed={m.fromAddr} />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className={`min-w-0 flex-1 truncate ${unread ? "font-semibold text-gray-900" : "text-gray-700"}`}>
                  {m.fromName || m.fromAddr}
                  {m.threadCount > 1 && <span className="ml-1 font-normal text-gray-500">{m.threadCount}</span>}
                </span>
                {m.hasAttachments && <span className="text-gray-400" title="Has attachments">📎</span>}
                <span className={`shrink-0 text-xs ${unread ? "font-semibold text-gray-800" : "text-gray-500"}`}>
                  {until ? `⏰ ${shortDate(until)}` : shortDate(m.date)}
                </span>
              </div>
              <div className="flex items-center gap-1">
                {chips.map((id) => (
                  <LabelChip key={id} remoteId={id} />
                ))}
                <span className={`min-w-0 truncate ${unread ? "font-medium text-gray-900" : "text-gray-700"}`}>
                  {m.subject || "(no subject)"}
                </span>
              </div>
              <div className="truncate text-xs text-gray-500">{m.snippet}</div>
            </div>
          </li>
        );
      })}
      {!search && !serverSearch && (
        <li className="flex justify-center p-3">
          {fetching ? (
            <span className="inline-flex items-center gap-2 text-xs text-gray-500">
              <Spinner size={14} className="text-blue-600" /> Loading more…
            </span>
          ) : hasMore ? (
            <button className="btn text-xs" onClick={() => void loadMore()}>
              Load more
            </button>
          ) : (
            <span className="text-xs text-gray-400">No more messages</span>
          )}
        </li>
      )}
    </ul>
  );
}
