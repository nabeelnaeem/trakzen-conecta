import { useEffect, useState } from "react";
import { MailView } from "./features/mail/MailView";
import { ChatView } from "./features/chat/ChatView";
import { SettingsView } from "./features/settings/SettingsView";
import { useMail } from "./features/mail/store";
import { useChat } from "./features/chat/store";
import { restoreZoom, setZoom, zoomStep } from "./lib/zoom";
import { app as appIpc } from "./lib/ipc";
import { isDark, onTheme, toggleDark } from "./lib/theme";
import { Mail, MessageSquare, Moon, Settings, Sun, type LucideIcon } from "lucide-react";
import { getLastTab, getRailOrder, getStartIn, setLastTab, setRailOrder, type RailItem } from "./lib/prefs";
import { onNavigate } from "./lib/navigate";

type Tab = "mail" | "chat" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>(() => {
    const s = getStartIn();
    return s === "last" ? getLastTab() : s;
  });
  const [order, setOrder] = useState<RailItem[]>(getRailOrder());
  const [dragging, setDragging] = useState<RailItem | null>(null);
  useEffect(() => {
    if (tab === "mail" || tab === "chat") setLastTab(tab);
  }, [tab]);
  useEffect(() => onNavigate((t) => setTab(t)), []);
  const reorder = (target: RailItem) => {
    if (!dragging || dragging === target) return;
    const next = order.filter((x) => x !== dragging);
    next.splice(next.indexOf(target), 0, dragging);
    setOrder(next);
    setRailOrder(next);
  };
  const rail: Record<RailItem, { label: string; icon: LucideIcon; badge: number; shortcut: string }> = {
    mail: { label: "Mail", icon: Mail, badge: 0, shortcut: "Ctrl+1" },
    chat: { label: "Chat", icon: MessageSquare, badge: 0, shortcut: "Ctrl+2" },
  };
  const mailUnread = useMail((s) => s.unread);
  const chatUnread = useChat((s) => s.peers.reduce((n, p) => n + p.unread, 0));
  const initMail = useMail((s) => s.init);
  const initChat = useChat((s) => s.init);
  const [dark, setDark] = useState(isDark());
  useEffect(() => onTheme((p) => setDark(isDark(p))), []);

  // Both engines start at launch so badges are live regardless of tab.
  useEffect(() => {
    void initMail();
    void initChat();
  }, [initMail, initChat]);

  // Tray tooltip / window title / taskbar badge follow the unread total.
  useEffect(() => {
    void appIpc.setBadge(mailUnread + chatUnread);
  }, [mailUnread, chatUnread]);

  // Ctrl+1/2/, switch tabs; Ctrl +/-/0 and Ctrl+wheel zoom.
  useEffect(() => {
    void restoreZoom();
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      if (e.key === "1") setTab(getRailOrder()[0]);
      else if (e.key === "2") setTab(getRailOrder()[1]);
      else if (e.key === ",") setTab("settings");
      else if (e.key === "=" || e.key === "+") {
        e.preventDefault();
        void zoomStep(1);
      } else if (e.key === "-") {
        e.preventDefault();
        void zoomStep(-1);
      } else if (e.key === "0") {
        e.preventDefault();
        void setZoom(1);
      }
    };
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      void zoomStep(e.deltaY < 0 ? 1 : -1);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", onWheel);
    };
  }, []);

  return (
    <div className="flex h-full">
      <nav className="flex w-16 shrink-0 flex-col items-center gap-1 border-r border-gray-200 bg-gray-100 py-2">
        <div className="mb-2 flex h-9 w-9 items-center justify-center rounded-xl bg-blue-600 text-on-accent shadow-sm" title="Trakzen Conecta">
          <svg viewBox="0 0 512 512" width="22" height="22" aria-hidden>
            <path d="M116 128h280c26.5 0 48 21.5 48 48v168c0 26.5-21.5 48-48 48H236l-76 62c-9.8 8-24.4 1-24.4-11.6V392H116c-26.5 0-48-21.5-48-48V176c0-26.5 21.5-48 48-48z" fill="#fff" />
            <rect x="136" y="188" width="240" height="152" rx="22" fill="#1d4ed8" />
            <path d="M158 214l98 74 98-74" fill="none" stroke="#fff" strokeWidth="22" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </div>
        {order.map((k) => (
          <div
            key={k}
            draggable
            onDragStart={() => setDragging(k)}
            onDragOver={(e) => e.preventDefault()}
            onDrop={() => reorder(k)}
            onDragEnd={() => setDragging(null)}
            className={dragging === k ? "opacity-50" : ""}
          >
            <NavButton
              label={rail[k].label}
              icon={rail[k].icon}
              active={tab === k}
              badge={k === "mail" ? mailUnread : chatUnread}
              onClick={() => setTab(k)}
              shortcut={rail[k].shortcut}
            />
          </div>
        ))}
        <div className="flex-1" />
        <button
          className="mb-1 flex h-9 w-9 items-center justify-center rounded-lg text-gray-600 hover:bg-gray-200"
          title={dark ? "Switch to light mode" : "Switch to dark mode"}
          onClick={toggleDark}
        >
          {dark ? <Sun size={18} /> : <Moon size={18} />}
        </button>
        <NavButton label="Settings" icon={Settings} active={tab === "settings"} onClick={() => setTab("settings")} shortcut="Ctrl+," />
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
  icon: LucideIcon;
  active: boolean;
  badge?: number;
  onClick: () => void;
  shortcut: string;
}) {
  return (
    <button
      onClick={onClick}
      title={`${label} (${shortcut})`}
      className={`relative flex h-12 w-12 flex-col items-center justify-center gap-0.5 rounded-xl transition-colors ${
        active ? "bg-white text-blue-700 shadow-sm ring-1 ring-gray-200" : "text-gray-600 hover:bg-gray-200"
      }`}
    >
      {(() => {
        const Icon = icon;
        return <Icon size={20} strokeWidth={1.75} aria-hidden />;
      })()}
      <span className="text-[9px] leading-none">{label}</span>
      {badge ? (
        <span className="absolute top-1 right-1 min-w-[16px] rounded-full bg-blue-600 px-1 text-center text-[9px] leading-4 text-on-accent">
          {badge > 99 ? "99+" : badge}
        </span>
      ) : null}
    </button>
  );
}
