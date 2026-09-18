import { initials } from "../../lib/format";

const PALETTE = [
  "#2563eb", "#7c3aed", "#db2777", "#dc2626", "#ea580c", "#ca8a04",
  "#16a34a", "#0d9488", "#0891b2", "#4f46e5", "#9333ea", "#be185d",
];

/** Initials on a colour picked deterministically from the address. */
export function Avatar({ name, seed, size = 36 }: { name: string; seed: string; size?: number }) {
  let h = 0;
  for (const c of seed.toLowerCase()) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const bg = PALETTE[h % PALETTE.length];
  return (
    <div
      className="flex shrink-0 items-center justify-center rounded-full font-semibold text-on-accent"
      style={{ width: size, height: size, background: bg, fontSize: size * 0.36 }}
      aria-hidden
    >
      {initials(name)}
    </div>
  );
}
