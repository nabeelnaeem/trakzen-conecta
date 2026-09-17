import { useEffect, useRef, useState } from "react";
import { mail } from "../../lib/ipc";
import type { Contact } from "../../lib/types";

/**
 * Comma-separated recipient field with suggestions from the account's
 * address book (people you've exchanged mail with). Suggestions apply to
 * the token after the last comma; Enter/Tab/click inserts "Name <addr>".
 */
export function RecipientInput({
  accountId,
  value,
  onChange,
  placeholder,
  autoFocus,
}: {
  accountId: number;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
}) {
  const [items, setItems] = useState<Contact[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const timer = useRef<number | null>(null);
  const seq = useRef(0);

  const lastComma = value.lastIndexOf(",");
  const head = lastComma === -1 ? "" : value.slice(0, lastComma + 1);
  const token = value.slice(lastComma + 1).trim();

  useEffect(() => {
    if (timer.current) window.clearTimeout(timer.current);
    if (token.length < 1 || token.includes("<")) {
      setItems([]);
      setOpen(false);
      return;
    }
    const mine = ++seq.current;
    timer.current = window.setTimeout(() => {
      mail
        .suggestContacts(accountId, token)
        .then((res) => {
          if (mine !== seq.current) return;
          setItems(res);
          setActive(0);
          setOpen(res.length > 0);
        })
        .catch(() => setItems([]));
    }, 120);
    return () => {
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, [token, accountId]);

  const pick = (c: Contact) => {
    const formatted = c.name ? `${c.name.replace(/[<>,"]/g, "")} <${c.email}>` : c.email;
    onChange(`${head}${head ? " " : ""}${formatted}, `);
    setOpen(false);
    setItems([]);
  };

  return (
    <div className="relative flex-1">
      <input
        className="w-full outline-none"
        autoFocus={autoFocus}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        onBlur={() => window.setTimeout(() => setOpen(false), 150)}
        onFocus={() => items.length > 0 && setOpen(true)}
        onKeyDown={(e) => {
          if (!open) return;
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setActive((a) => (a + 1) % items.length);
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setActive((a) => (a - 1 + items.length) % items.length);
          } else if (e.key === "Enter" || e.key === "Tab") {
            e.preventDefault();
            pick(items[active]);
          } else if (e.key === "Escape") {
            setOpen(false);
          }
        }}
      />
      {open && (
        <ul className="absolute left-0 top-full z-20 mt-1 w-full max-w-md overflow-hidden rounded-md border border-gray-200 bg-white py-1 shadow-lg">
          {items.map((c, i) => (
            <li
              key={c.email}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(c);
              }}
              onMouseEnter={() => setActive(i)}
              className={`flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm ${
                i === active ? "bg-blue-50" : ""
              }`}
            >
              <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-gray-200 text-[10px] font-semibold text-gray-700">
                {(c.name || c.email).slice(0, 1).toUpperCase()}
              </span>
              <span className="min-w-0 flex-1 truncate">
                {c.name && <span className="font-medium">{c.name} </span>}
                <span className="text-gray-500">{c.email}</span>
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
