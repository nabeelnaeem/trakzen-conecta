import { useEffect } from "react";
import { useMail } from "./store";

const typing = (el: EventTarget | null) => {
  const t = el as HTMLElement | null;
  if (!t) return false;
  const tag = t.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable;
};

/** Gmail's keyboard shortcuts, the ones people actually use. */
export function useMailShortcuts(searchRef: React.RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = useMail.getState();
      if (s.composer || s.filterEditor) {
        if (e.key === "Escape" && s.filterEditor) s.closeFilterEditor();
        return;
      }
      if (typing(e.target)) {
        if (e.key === "Escape") (e.target as HTMLElement).blur();
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const open = s.openId;
      const latest = s.thread[s.thread.length - 1];
      const stop = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      switch (e.key) {
        case "j":
          stop();
          void s.openNext(1);
          break;
        case "k":
          stop();
          void s.openNext(-1);
          break;
        case "o":
        case "Enter":
          if (open === null && s.messages.length) {
            stop();
            void s.open(s.messages[0].id);
          }
          break;
        case "e":
          stop();
          void s.act("archive");
          break;
        case "#":
        case "Delete":
          stop();
          void s.act("trash");
          break;
        case "!":
          stop();
          void s.act("spam");
          break;
        case "U":
          stop();
          void s.act("unread");
          break;
        case "I":
          stop();
          void s.act("read");
          break;
        case "s":
          if (latest) {
            stop();
            void s.toggleStar(latest);
          }
          break;
        case "r":
          if (latest) {
            stop();
            void s.openCompose("reply", latest.id);
          }
          break;
        case "a":
          if (latest) {
            stop();
            void s.openCompose("reply-all", latest.id);
          }
          break;
        case "f":
          if (latest) {
            stop();
            void s.openCompose("forward", latest.id);
          }
          break;
        case "c":
          stop();
          void s.openCompose();
          break;
        case "/":
          stop();
          searchRef.current?.focus();
          break;
        case "x":
          if (open !== null) {
            stop();
            s.toggleSelect(open);
          }
          break;
        case "*":
          stop();
          s.selectAll(s.selected.length === 0);
          break;
        case "Escape":
          if (s.selected.length) s.selectAll(false);
          else void s.open(null);
          break;
        case "u":
          stop();
          void s.open(null);
          break;
        default:
          return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [searchRef]);
}
