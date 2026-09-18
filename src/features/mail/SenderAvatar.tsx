import { useEffect, useState } from "react";
import { Avatar } from "./Avatar";
import { getSenderLogos } from "../../lib/prefs";

/**
 * Sender avatar: when the user has opted in, try Gravatar (SHA-256 of the
 * address) then the domain's favicon; otherwise, or if both miss, show
 * initials. Results are cached per address for the session so the list
 * does not re-probe on every render.
 */
const cache = new Map<string, string | null>();
const pending = new Map<string, Promise<string | null>>();

async function sha256Hex(s: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s));
  return Array.from(new Uint8Array(buf))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function probe(url: string): Promise<boolean> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img.naturalWidth > 1);
    img.onerror = () => resolve(false);
    img.src = url;
  });
}

async function lookup(email: string): Promise<string | null> {
  const addr = email.trim().toLowerCase();
  if (cache.has(addr)) return cache.get(addr)!;
  if (pending.has(addr)) return pending.get(addr)!;
  const run = (async () => {
    const gravatar = `https://www.gravatar.com/avatar/${await sha256Hex(addr)}?d=404&s=80`;
    if (await probe(gravatar)) return gravatar;
    const domain = addr.split("@")[1];
    if (domain) {
      // Strip common mailer subdomains so newsletters resolve to the brand.
      const root = domain.replace(/^(mail|email|e|news|newsletter|info|notifications?|no-?reply|hello|updates?|alerts?|support)\./, "");
      for (const d of [root, domain]) {
        const icon = `https://icons.duckduckgo.com/ip3/${d}.ico`;
        if (await probe(icon)) return icon;
      }
    }
    return null;
  })();
  pending.set(addr, run);
  const result = await run.catch(() => null);
  cache.set(addr, result);
  pending.delete(addr);
  return result;
}

export function SenderAvatar({ name, email, size = 36 }: { name: string; email: string; size?: number }) {
  const [src, setSrc] = useState<string | null>(() => (getSenderLogos() ? (cache.get(email.toLowerCase()) ?? null) : null));
  useEffect(() => {
    if (!getSenderLogos() || !email.includes("@")) return;
    let alive = true;
    void lookup(email).then((u) => alive && setSrc(u));
    return () => {
      alive = false;
    };
  }, [email]);
  if (!src) return <Avatar name={name || email} seed={email} size={size} />;
  return (
    <img
      src={src}
      alt=""
      width={size}
      height={size}
      className="shrink-0 rounded-full bg-white object-cover ring-1 ring-gray-200"
      style={{ width: size, height: size }}
      onError={() => setSrc(null)}
    />
  );
}
