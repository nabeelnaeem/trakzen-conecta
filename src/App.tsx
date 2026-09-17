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

  // Ctrl+1/2/3 switch tabs.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      if (e.key === "1") setTab("mail");
      else if (e.key === "2") setTab("chat");
      else if (e.key === ",") setTab("settings");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full">
      <nav className="flex w-16 shrink-0 flex-col items-center gap-1 border-r border-gray-200 bg-gray-100 py-2">
        <div className="mb-2 flex h-9 w-9 items-center justify-center rounded-xl bg-blue-600 text-white shadow-sm" title="Trakzen Conecta">
          <svg viewBox="0 0 512 512" width="22" height="22" aria-hidden>
            <path d="M116 128h280c26.5 0 48 21.5 48 48v168c0 26.5-21.5 48-48 48H236l-76 62c-9.8 8-24.4 1-24.4-11.6V392H116c-26.5 0-48-21.5-48-48V176c0-26.5 21.5-48 48-48z" fill="#fff" />
            <rect x="136" y="188" width="240" height="152" rx="22" fill="#1d4ed8" />
            <path d="M158 214l98 74 98-74" fill="none" stroke="#fff" strokeWidth="22" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </div>
        <NavButton label="Mail" icon="✉" active={tab === "mail"} badge={mailUnread} onClick={() => setTab("mail")} shortcut="Ctrl+1" />
        <NavButton label="Chat" icon="💬" active={tab === "chat"} badge={chatUnread} onClick={() => setTab("chat")} shortcut="Ctrl+2" />
        <div className="flex-1" />
        <NavButton label="Settings" icon="⚙" active={tab === "settings"} onClick={() => setTab("settings")} shortcut="Ctrl+," />
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
  shortcut,
}: {
  label: string;
  icon: string;
  active: boolean;
  badge?: number;
  onClick: () => void;
  shortcut: string;
}) {
  return (
    <button
      onClick={onClick}
      title={`${label} (${shortcut})`}
      className={`relative flex h-12 w-12 flex-col items-center justify-center gap-0.5 rounded-xl text-lg transition-colors ${
        active ? "bg-white text-blue-700 shadow-sm ring-1 ring-gray-200" : "text-gray-600 hover:bg-gray-200"
      }`}
    >
      <span aria-hidden className="leading-none">{icon}</span>
      <span className="text-[9px] leading-none">{label}</span>
      {badge ? (
        <span className="absolute top-1 right-1 min-w-[16px] rounded-full bg-blue-600 px-1 text-center text-[9px] leading-4 text-white">
          {badge > 99 ? "99+" : badge}
        </span>
      ) : null}
    </button>
  );
}
