// Prints the CSP hash for the inline script the mail reading pane injects.
// Paste the output into `script-src` in src-tauri/tauri.conf.json.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const src = readFileSync(new URL("../src/features/mail/frame.ts", import.meta.url), "utf8");
const m = src.match(/export const FRAME_SCRIPT =\s*"((?:[^"\\]|\\.)*)";/);
if (!m) throw new Error("FRAME_SCRIPT not found");
const script = JSON.parse(`"${m[1]}"`);
console.log(`'sha256-${createHash("sha256").update(script).digest("base64")}'`);
