import { useMail } from "./store";
import { shortDate } from "../../lib/format";

export function MessageList() {
  const { messages, selectedId, select, toggleStar, search, folder, syncing, activeAccountId } =
    useMail();

  if (messages.length === 0) {
    const syncingNow = activeAccountId !== null && !!syncing[activeAccountId];
    return (
      <div className="flex flex-1 items-center justify-center p-6 text-center text-sm text-gray-500">
        {search
          ? "No messages match."
          : syncingNow
            ? "Fetching messages…"
            : `Nothing in ${folder}.`}
      </div>
    );
  }

  return (
    <ul className="flex-1 overflow-y-auto">
      {messages.map((m) => {
        const selected = m.id === selectedId;
        return (
          <li
            key={m.id}
            onClick={() => void select(m.id)}
            className={`cursor-pointer border-b border-gray-100 px-3 py-2 ${
              selected ? "bg-blue-50" : "hover:bg-gray-50"
            } ${m.isRead ? "" : "bg-white"}`}
          >
            <div className="flex items-center gap-2">
              <button
                className={`shrink-0 text-base leading-none ${
                  m.isStarred ? "text-amber-500" : "text-gray-300 hover:text-gray-500"
                }`}
                onClick={(e) => {
                  e.stopPropagation();
                  void toggleStar(m);
                }}
                aria-label={m.isStarred ? "Unstar" : "Star"}
              >
                ★
              </button>
              <span
                className={`min-w-0 flex-1 truncate ${m.isRead ? "text-gray-700" : "font-semibold text-gray-900"}`}
              >
                {m.fromName || m.fromAddr}
              </span>
              {m.hasAttachments && <span className="text-gray-400" title="Has attachments">📎</span>}
              <span className={`shrink-0 text-xs ${m.isRead ? "text-gray-500" : "font-semibold text-gray-800"}`}>
                {shortDate(m.date)}
              </span>
            </div>
            <div className={`truncate pl-6 ${m.isRead ? "text-gray-700" : "font-medium text-gray-900"}`}>
              {m.subject || "(no subject)"}
            </div>
            <div className="truncate pl-6 text-xs text-gray-500">{m.snippet}</div>
          </li>
        );
      })}
    </ul>
  );
}
