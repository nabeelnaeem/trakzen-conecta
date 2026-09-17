import { useEffect, useState } from "react";
import { MailView } from "./features/mail/MailView";
import { ChatView } from "./features/chat/ChatView";
import { SettingsView } from "./features/settings/SettingsView";
import { useMail } from "./features/mail/store";
import { useChat } from "./features/chat/store";

type Tab = "mail" | "chat" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("mail");
  const mailUnread = useMail((s) => s.unread);
  const chatUnread = useChat((s) => s.peers.reduce((n, p) => n + p.unread, 0));
  const initMail = useMail((s) => s.init);
  const initChat = useChat((s) => s.init);

  // Both engines start at launch so badges are live regardless of tab.
  useEffect(() => {
    void initMail();
    void initChat();
  }, [initMail, initChat]);

  return (
    <div className="flex h-full">
      <nav className="flex w-14 shrink-0 flex-col items-center gap-1 border-r border-gray-200 bg-gray-100 py-2">
        <NavButton label="Mail" icon="✉" active={tab === "mail"} badge={mailUnread} onClick={() => setTab("mail")} />
        <NavButton label="Chat" icon="💬" active={tab === "chat"} badge={chatUnread} onClick={() => setTab("chat")} />
        <div className="flex-1" />
        <NavButton label="Settings" icon="⚙" active={tab === "settings"} onClick={() => setTab("settings")} />
      </nav>
      {/* Views stay mounted so switching tabs is instant and keeps scroll position. */}
      <main className="min-w-0 flex-1">
        <div className={tab === "mail" ? "h-full" : "hidden"}>
          <MailView />
        </div>
        <div className={tab === "chat" ? "h-full" : "hidden"}>
          <ChatView />
        </div>
        <div className={tab === "settings" ? "h-full" : "hidden"}>
          {tab === "settings" && <SettingsView />}
        </div>
      </main>
    </div>
  );
}

function NavButton({
  label,
  icon,
  active,
  badge,
  onClick,
}: {
  label: string;
  icon: string;
  active: boolean;
  badge?: number;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={label}
      className={`relative flex h-11 w-11 flex-col items-center justify-center rounded-lg text-lg ${
        active ? "bg-white text-blue-700 shadow-sm" : "text-gray-600 hover:bg-gray-200"
      }`}
    >
      <span aria-hidden>{icon}</span>
      <span className="text-[9px] leading-none">{label}</span>
      {badge ? (
        <span className="absolute top-0.5 right-0.5 rounded-full bg-blue-600 px-1 text-[9px] leading-4 text-white">
          {badge > 99 ? "99+" : badge}
        </span>
      ) : null}
    </button>
  );
}
