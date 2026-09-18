import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { Label } from "../../lib/types";

/**
 * Gmail nests labels with "/" in the name ("Work/Clients/Acme"). Build a
 * tree from the flat list, remember which branches are collapsed, and
 * render each node with its own colour dot and unread count.
 */
interface Node {
  name: string;
  path: string;
  label: Label | null;
  children: Node[];
  unread: number;
}

function buildTree(labels: Label[]): Node[] {
  const root: Node = { name: "", path: "", label: null, children: [], unread: 0 };
  for (const l of labels) {
    const parts = l.name.split("/");
    let cur = root;
    let path = "";
    for (const part of parts) {
      path = path ? `${path}/${part}` : part;
      let next = cur.children.find((c) => c.name === part);
      if (!next) {
        next = { name: part, path, label: null, children: [], unread: 0 };
        cur.children.push(next);
      }
      cur = next;
    }
    cur.label = l;
  }
  const sum = (n: Node): number => {
    n.unread = (n.label?.unread ?? 0) + n.children.reduce((a, c) => a + sum(c), 0);
    n.children.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
    return n.unread;
  };
  sum(root);
  return root.children;
}

const KEY = "tc.collapsedLabels";
const loadCollapsed = (): Set<string> => {
  try {
    return new Set(JSON.parse(localStorage.getItem(KEY) ?? "[]") as string[]);
  } catch {
    return new Set();
  }
};

export function LabelTree({ labels, active, onPick }: { labels: Label[]; active: string | null; onPick: (remoteId: string) => void }) {
  const tree = useMemo(() => buildTree(labels), [labels]);
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);
  const toggle = (path: string) => {
    const next = new Set(collapsed);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    setCollapsed(next);
    localStorage.setItem(KEY, JSON.stringify(Array.from(next)));
  };

  const render = (nodes: Node[], depth: number) =>
    nodes.map((n) => {
      const isActive = n.label !== null && n.label.remoteId === active;
      const hasKids = n.children.length > 0;
      const closed = collapsed.has(n.path);
      return (
        <div key={n.path}>
          <div
            className={`flex w-full items-center gap-1.5 rounded-md py-1 pr-2 text-left text-sm ${
              isActive ? "bg-blue-100 font-medium text-blue-900" : "text-gray-700 hover:bg-gray-200"
            }`}
            style={{ paddingLeft: 12 + depth * 14 }}
          >
            {hasKids ? (
              <button className="-ml-1 rounded p-0.5 text-gray-400 hover:text-gray-700" onClick={() => toggle(n.path)} aria-label={closed ? "Expand" : "Collapse"}>
                {closed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
              </button>
            ) : (
              <span className="w-3" />
            )}
            <button
              className="flex min-w-0 flex-1 items-center gap-2"
              onClick={() => n.label && onPick(n.label.remoteId)}
              disabled={!n.label}
              title={n.label ? `${n.label.total} message${n.label.total === 1 ? "" : "s"}` : undefined}
            >
              <span className="h-2.5 w-2.5 shrink-0 rounded-sm" style={{ background: n.label?.bgColor ?? "var(--color-gray-400)" }} />
              <span className="min-w-0 flex-1 truncate">{n.name}</span>
              {(closed ? n.unread : (n.label?.unread ?? 0)) > 0 && (
                <span className="text-xs font-semibold text-gray-700">{closed ? n.unread : n.label?.unread}</span>
              )}
            </button>
          </div>
          {hasKids && !closed && render(n.children, depth + 1)}
        </div>
      );
    });

  return <>{render(tree, 0)}</>;
}
