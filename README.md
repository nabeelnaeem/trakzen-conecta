# Trakzen Conecta

A small, fast desktop app for two things people do all day: read and answer
email, and talk to the machine next door.

- **Mail** – a lightweight Gmail client built so other providers can be
  added. The message list renders from a local SQLite cache the instant the
  window opens; syncing runs in the background through Gmail's incremental
  history API, so a refresh is one tiny request instead of reloading a web
  app.
- **Chat + files** – peer-to-peer messaging over your local network. No
  server, no account: machines running the app find each other
  automatically, and files go straight from one machine to the other.

Built with [Tauri 2](https://tauri.app) (Rust backend, system webview) and
React. Storage is a single SQLite file, `trakzen-conecta.db`. Licensed under
[MIT](LICENSE).

## Features

### Mail (Gmail, or any IMAP/SMTP account)
- Gmail through its REST API (OAuth, labels, filters, incremental sync) or
  any other provider over IMAP/SMTP with a password or app password
  (Settings → Mail → *IMAP (any provider)*)
- Instant inbox from the local cache; background sync every 60 s
  (configurable) and on window focus; new-mail notifications with sound
- Conversation view with the newest and unread messages expanded
- Inbox tabs (Primary / Social / Promotions / Updates / Forums), labels with
  colours and unread counts, label add/remove, new labels
- Reply, reply-all, forward with proper threading; attachments; recipient
  suggestions from your mail history; signature
- Rich-text composer (bold, italic, underline, lists, quotes, links) with a
  plain-text part sent alongside; drafts reopen with your text editable
- Drafts autosaved to Gmail; undo send (configurable delay)
- Archive, trash, spam / not spam, star, mark read/unread — singly, in bulk
  with multi-select, or across a whole folder ("select all N conversations")
- Filters: create and delete Gmail filters in-app, optionally applied to
  existing mail; "filter messages like this"
- Snooze (local), server-side search with Gmail operators, Gmail keyboard
  shortcuts (`j` `k` `e` `#` `!` `r` `a` `f` `c` `s` `x` `*` `/`)
- HTML mail sanitised and rendered in a sandboxed frame; embedded (`cid:`)
  images shown inline; remote images on or off; links open in the system
  browser

### Chat (LAN)
- Automatic peer discovery over mDNS; manual IP and QR / `conecta://`
  pairing link as fallback
- Text with Markdown (bold, italic, strikethrough, inline code, lists,
  quotes, links) and a Slack-style code-block mode with language label and
  Copy button
- Paste screenshots, drag-and-drop or attach files; they wait in the compose
  box until you press Enter; inline image previews; progress bars
- Typing indicator, delivered vs read ticks, reply/quote, edit, reactions,
  pins, in-chat search (Ctrl+F), peer switcher (Ctrl+K)
- Group chats: anyone in the group can add or remove members, rename it or
  leave; messages show who wrote them and everything (files included) goes
  to every member
- Messages and files sent while a peer is offline are queued and delivered
  when it returns; an interrupted file transfer resumes from where it
  stopped; reconnects back off exponentially and try every known address
- Incoming files are offered first: accept, decline, or always accept from
  that peer (or turn the prompt off in Settings). An unanswered offer stays
  open for seven days, even across restarts
- Delete for me / delete for everyone, clear chat on this machine; files the
  app saved are removed with their messages; storage clean-up in Settings
- Seven notification sounds, chosen separately for mail and chat

### App
- System tray (close hides, tray menu quits), unread badge on tray tooltip,
  window title and taskbar; start at login; text zoom (Ctrl +/−/0)
- Settings → About shows the version, build id and build timestamp

## Requirements

**To run the installers**

| OS | Needs |
|---|---|
| Windows 10 / 11 (x64) | WebView2 runtime (already present on Windows 10/11) |
| Ubuntu 22.04 / 24.04, Debian 12 (x64) | `libwebkit2gtk-4.1`, GTK 3, a Secret Service (GNOME Keyring / KWallet) for the Gmail token — all present on a desktop install. Tray icon on GNOME needs `gnome-shell-extension-appindicator`. |
| macOS 12+ (Intel or Apple silicon) | nothing extra |

**For Gmail:** a Google Cloud OAuth client of type *Desktop app* (free, ~5
minutes, steps below). The app ships no Google credentials.

**For chat:** both machines on the same network with TCP port **47800**
(configurable) allowed through their firewalls. Multicast (mDNS) must be
permitted for automatic discovery; otherwise add peers by IP.

## Installing

Prebuilt installers are attached to each
[GitHub release](https://github.com/nabeelnaeem/trakzen-conecta/releases):

| File | Platform |
|---|---|
| `Trakzen.Conecta_<version>_x64-setup.exe` | Windows (per-user, no admin needed) |
| `Trakzen.Conecta_<version>_x64_en-US.msi` | Windows (MSI, for deployment tools) |
| `Trakzen.Conecta_<version>_amd64.deb` | Ubuntu / Debian |
| `Trakzen.Conecta_<version>_amd64.AppImage` | any x64 Linux |
| `Trakzen.Conecta-<version>-1.x86_64.rpm` | Fedora / openSUSE |
| `Trakzen.Conecta_<version>_universal.dmg` | macOS |

### Windows
Run the `.exe`. The installer is not code-signed yet, so SmartScreen shows
"unknown publisher" the first time — choose *More info → Run anyway*. On first
launch allow the app through Windows Firewall for private networks (chat).

### Ubuntu / Debian
```sh
sudo apt install ./Trakzen.Conecta_<version>_amd64.deb
trakzen-conecta            # or launch from the app menu
```
If `ufw` is enabled: `sudo ufw allow 47800/tcp`. For the tray icon on GNOME:
`sudo apt install gnome-shell-extension-appindicator`, enable it in
*Extensions*, log out and in. The AppImage needs `libfuse2`.

### macOS
Open the `.dmg` and drag the app to Applications. It is not notarised yet;
right-click → Open the first time.

Upgrading: install the new version over the old one. Your database, settings
and Gmail token are kept.

## Setting up Gmail

Every user (or team) creates their own OAuth client, which keeps the project
free of secrets.

1. Open https://console.cloud.google.com and create a project (or pick one).
2. **APIs & Services → Library**, search for *Gmail API*, enable it.
3. **Google Auth Platform** (older consoles: *OAuth consent screen*): choose
   **External**, fill in the app name and your email, create.
4. **Audience → Test users**: add the Gmail addresses that will sign in. In
   *Testing* status only listed users can sign in and tokens expire after
   7 days; **Publish app** removes both limits (a homepage and privacy-policy
   URL are required — this repository's URLs work).
5. **Clients → Create client**, application type **Desktop app**. Copy the
   client ID and client secret.
6. In Trakzen Conecta open **Settings → Gmail**, paste both values, save.
7. **Mail → Connect Gmail**. Your browser opens Google's consent page; with an
   unverified app click *Advanced → Go to Trakzen Conecta*, approve, and
   return to the app. The inbox starts filling within seconds.

The app requests two scopes: `gmail.modify` (read, send, label changes,
trash – it cannot permanently delete mail) and `gmail.settings.basic`
(creating and deleting filters). Refresh tokens are stored in the operating
system's credential store (Windows Credential Manager, macOS Keychain,
Secret Service on Linux), never in the database. The Gmail API is free;
the app stays far inside its quota.

## Using chat

1. Run the app on both machines. Each listens on TCP **47800** (change it in
   Settings → Chat; it takes effect immediately) and advertises itself on
   the LAN.
2. The other machine appears under **Nearby** in the Chat tab — click **Add**.
   If discovery is blocked, use *Add peer by IP or pairing link*; the ▦
   button shows your own address as a QR code / `conecta://` link.
3. Type to chat. Enter sends, Shift+Enter adds a line, `{ }` opens code mode
   (Ctrl+Enter sends), paste or drop files to attach them. Received files
   land in `Downloads/Trakzen Conecta` (configurable) once you accept them.
4. **+ New group** picks peers that have connected at least once. The
   group's member list is kept in sync between members, so someone who was
   offline during a change catches up when they reconnect.

Traffic is plain TCP on your LAN. Do not expose the port to the internet;
end-to-end encryption and a relay for remote use are on the roadmap.

## Building from source

Requirements:

- Rust stable 1.80+ – https://rustup.rs
- Node.js 20+ and pnpm 10 – `corepack enable`
- Tauri prerequisites – https://tauri.app/start/prerequisites/
  - Ubuntu / Debian:
    ```sh
    sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
      libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
      libdbus-1-dev pkg-config
    ```
  - Windows: Visual Studio Build Tools (C++ workload) and the WebView2
    runtime
  - macOS: Xcode command-line tools

```sh
git clone https://github.com/nabeelnaeem/trakzen-conecta.git
cd trakzen-conecta
pnpm install
pnpm tauri dev                 # run with hot reload (dev server on port 1470)
pnpm tauri build               # installers under src-tauri/target/release/bundle/
pnpm tauri build --bundles deb # just one bundle type
cargo test --manifest-path src-tauri/Cargo.toml
```

The first Rust build takes several minutes; later ones are incremental.
pnpm 12 enforces a package "cooldown" that can reject freshly published
dependencies; if `pnpm install` fails with
`ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION`, create a local
`pnpm-workspace.yaml` containing `minimumReleaseAge: 0` (it is git-ignored).

Releases are built by the *Release* GitHub Actions workflow, started by
hand from `main` with the version number (Actions → Release → Run
workflow); it tags the commit and publishes the installers. macOS is
built only when asked for.

### Testing chat on one machine

```sh
TRAKZEN_CONECTA_DATA_DIR=/tmp/conecta-b TRAKZEN_CONECTA_CHAT_PORT=47801 pnpm tauri dev
```

Then add `127.0.0.1` port `47801` as a peer from the first instance.

## How it is put together

```
src/                    React + TypeScript UI (Vite, Tailwind)
  features/mail         folders, list, thread view, composer, filters
  features/chat         peers, discovery, conversation, transfers
  features/settings
  lib/ipc.ts            typed wrappers around Tauri commands and events
src-tauri/src/
  db/                   SQLite connection + migrations (rusqlite, bundled)
  mail/
    mod.rs              MailProvider trait – the seam for new providers
    gmail/              OAuth (PKCE + loopback), REST client, sync, paging
    store.rs            local message cache, labels, contacts, snoozes
    compose.rs          RFC 5322 building, reply/forward drafts
    sanitize.rs         HTML mail cleaning (ammonia)
  chat/
    protocol.rs         framed TCP wire format
    engine.rs           listener, connections, queue, transfers, groups
    discovery.rs        mDNS advertise + browse
    store.rs            peers + history
  secrets.rs            OS credential store access
  settings.rs           key/value settings in SQLite
```

Design notes:

- **Local first.** Every list and body the UI shows comes from SQLite. The
  network only runs in background tasks that emit events which the stores
  react to.
- **Gmail sync** pulls recent metadata once, records the mailbox `historyId`,
  and from then on applies only the changes the history API reports. Label
  changes in that feed are applied without re-downloading messages; requests
  are paced and retried so the per-user quota is never hit.
- **HTML mail** is sanitised in Rust and rendered in a sandboxed iframe whose
  only script forwards link clicks to the system browser.
- **Chat wire format** is `kind:u8 | len:u32 | payload`. Control frames are
  JSON, file chunks are binary. A file transfer opens its own TCP connection
  so a large upload never delays messages. The receiver keeps partial files
  and tells a re-offering sender how many bytes it already has, so
  transfers resume rather than restart. Unknown control frames are skipped,
  so builds with different feature sets keep talking.
- **Groups** have no server: every member holds the roster, and whoever
  changes it pushes a versioned copy to the others (and again whenever a
  connection comes up). Each member's delivery of a message is tracked
  separately, so retries go only to those who missed it.
- **Adding a provider** means implementing `MailProvider` in a new module and
  registering it in `Providers::provider_for`. The UI and store do not change.

## Roadmap

- End-to-end encryption for chat (Noise) with peer fingerprints, and peer
  authentication
- Optional relay server for chat outside the LAN
- Audio / video calls (WebRTC, signalling over the existing connection)
- Outlook / Microsoft 365 via Microsoft Graph
- Avatar images

Contributions and bug reports are welcome.

## License

[MIT](LICENSE) © Nabeel Naeem
