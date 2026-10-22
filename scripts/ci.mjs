// Local CI: the checks the Release workflow's `check` job runs, and more, on
// this machine. This is the gate before every push (.githooks/pre-push runs
// the default mode); Actions only runs when a release is started by hand.
// Works on Windows and Linux.
//
//   node scripts/ci.mjs           versions, CSP hash, install, typecheck,
//                                 frontend build, Rust tests
//   node scripts/ci.mjs --full    also build this platform's installers
//                                 (without updater artifacts)
//   node scripts/ci.mjs --lint    also cargo fmt --check and clippy -D warnings
//   node scripts/ci.mjs --list    print the steps without running them
//
// Exits non-zero on the first failing step.

import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";
import os from "node:os";

const args = process.argv.slice(2);
const known = ["--full", "--lint", "--list"];
const unknown = args.filter((a) => !known.includes(a));
if (unknown.length) {
  console.error(`Unknown option ${unknown.join(" ")}. Options: ${known.join(" ")}`);
  process.exit(2);
}
const full = args.includes("--full");
// Not in the default run yet: rustfmt has never been applied to the tree and
// clippy -D warnings fails on existing code. Move these up once both pass.
const lint = args.includes("--lint");

const root = new URL("../", import.meta.url);
const read = (f) => readFileSync(new URL(f, root), "utf8");

// Same test as the Release workflow, which refuses to build when the three
// files disagree with the version being released.
function checkVersions() {
  const v = JSON.parse(read("package.json")).version;
  console.log(`  package.json: ${v}`);
  const bad = [];
  for (const f of ["package.json", "src-tauri/tauri.conf.json"]) {
    if (!read(f).includes(`"version": "${v}"`)) bad.push(f);
  }
  if (!new RegExp(`^version = "${v.replace(/\./g, "\\.")}"`, "m").test(read("src-tauri/Cargo.toml"))) {
    bad.push("src-tauri/Cargo.toml");
  }
  if (bad.length) throw new Error(`not at ${v}: ${bad.join(", ")}`);
}

// The mail reading pane injects FRAME_SCRIPT inline; if its hash is missing
// from script-src the webview silently blocks it and the pane breaks only in
// the built app.
function checkCspHash() {
  const hash = execSync("node scripts/csp-hash.mjs", { cwd: root, encoding: "utf8" }).trim();
  const csp = JSON.parse(read("src-tauri/tauri.conf.json")).app?.security?.csp ?? "";
  const scriptSrc = csp.split(";").map((d) => d.trim()).find((d) => d.startsWith("script-src")) ?? "";
  console.log(`  FRAME_SCRIPT: ${hash}`);
  if (!scriptSrc.split(/\s+/).includes(hash)) {
    throw new Error(`script-src in src-tauri/tauri.conf.json lacks ${hash} (run node scripts/csp-hash.mjs)`);
  }
}

const steps = [
  ["Versions agree", checkVersions],
  ["CSP hash matches the mail frame script", checkCspHash],
  // A no-op when node_modules is current; fails if package.json changed
  // without pnpm-lock.yaml, as the release build would.
  ["Install frontend packages", "pnpm install --frozen-lockfile"],
  ["Typecheck", "pnpm typecheck"],
  // Tauri's generate_context! embeds dist/, so build it before cargo.
  ["Frontend build", "pnpm build"],
];
if (lint) {
  steps.push(
    ["Rust format", "cargo fmt --manifest-path src-tauri/Cargo.toml --check"],
    ["Clippy", "cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings"],
  );
}
// --locked fails if Cargo.lock is stale (say, a version bump that missed it)
// instead of quietly rewriting it.
steps.push(["Rust tests", "cargo test --manifest-path src-tauri/Cargo.toml --locked"]);
if (full) {
  steps.push(["Installers", "pnpm tauri build"]);
}

if (args.includes("--list")) {
  for (const [name, cmd] of steps) {
    console.log(`${name}: ${typeof cmd === "string" ? cmd : "(built in)"}`);
  }
  process.exit(0);
}

const started = Date.now();
for (const [name, cmd] of steps) {
  const t = Date.now();
  console.log(`\n▶ ${name}${typeof cmd === "string" ? `: ${cmd}` : ""}`);
  try {
    if (typeof cmd === "string") execSync(cmd, { cwd: root, stdio: "inherit" });
    else cmd();
  } catch (e) {
    if (typeof cmd !== "string") console.error(`  ${e.message}`);
    console.error(`\n✗ ${name} failed (${os.platform()})`);
    process.exit(1);
  }
  console.log(`✓ ${name} (${((Date.now() - t) / 1000).toFixed(1)} s)`);
}
console.log(`\nAll checks passed on ${os.platform()} in ${Math.round((Date.now() - started) / 1000)} s.`);
