# Trakzen Conecta

A small, fast desktop app for two things people do all day: read and answer
email, and talk to the machine next door.

- **Mail** – a lightweight client that starts with Gmail and is built so other
  providers can be added. The message list renders from a local SQLite cache
  the instant the window opens; syncing happens in the background using
  Gmail's incremental history API, so a refresh is one tiny request instead of
  reloading a web app.
- **Chat + files** – peer-to-peer messaging over your local network. No
  server, no account: every copy of the app listens on a port, you add
  colleagues by IP address, and files go straight from one machine to the
  other.

Built with [Tauri 2](https://tauri.app) (Rust backend, system webview) and
React. Storage is a single SQLite file, `trakzen-conecta.db`.

> Status: early. Gmail read / reply / forward and LAN chat / file transfer
> work; see [Roadmap](#roadmap) for what is still missing.

## Installing

Prebuilt installers are attached to each
[GitHub release](https://github.com/nabeelnaeem/trakzen-conecta/releases):
`.exe` for Windows, `.deb` and `.AppImage` for Ubuntu/Debian, `.dmg` for
macOS.

### Ubuntu / Debian

```sh
sudo apt install ./trakzen-conecta_*_amd64.deb
```

or make the AppImage executable and run it. Two things the app expects on
Linux:

- a Secret Service provider for storing the Gmail token (GNOME Keyring or
  KWallet – present on any desktop install);
- for the tray icon on GNOME, the AppIndicator extension:
  `sudo apt install gnome-shell-extension-appindicator`, then enable it in
  Extensions and log out/in once.

## Building from source

- Rust (stable, 1.80+) – https://rustup.rs
- Node.js 20+ and pnpm – `corepack enable` or `npm i -g pnpm`
- Platform prerequisites for Tauri: https://tauri.app/start/prerequisites/
  On Ubuntu 22.04/24.04 that is:

  ```sh
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file     libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev     libdbus-1-dev pkg-config
  ```

  On Windows: the WebView2 runtime (already on Windows 10/11) and the MSVC
  build tools.

```sh
pnpm install
pnpm tauri dev      # run with hot reload
pnpm tauri build    # installers under src-tauri/target/release/bundle/
```

The first Rust build takes a few minutes; later ones are incremental.

## Setting up Gmail

The app does not ship with Google API credentials – every user (or team)
creates their own OAuth client, which takes about five minutes and keeps the
project free of secrets.

1. Open https://console.cloud.google.com and create a project (or pick one).
2. **APIs & Services → Library**, search for *Gmail API*, enable it.
3. **APIs & Services → OAuth consent screen**: choose *External*, fill in the
   app name and your email, save. Under *Scopes* nothing needs adding here.
   Under *Test users* add the Gmail addresses you want to sign in with (while
   the consent screen is in "Testing" mode only listed users can sign in,
   which is fine for personal use).
4. **APIs & Services → Credentials → Create credentials → OAuth client ID**,
   application type **Desktop app**. Copy the client ID and client secret.
5. In Trakzen Conecta open **Settings**, paste both values, save.
6. Go to **Mail → Connect Gmail**. Your browser opens Google's consent page;
   after approving, the tab tells you to return to the app and the inbox
   starts filling.

The app requests the single `gmail.modify` scope: read, send, label changes
and moving to trash. It cannot permanently delete mail.

Refresh tokens are stored in the operating system's credential store
(Windows Credential Manager, macOS Keychain, Secret Service on Linux), not in
the database.

## Using chat

1. Both machines run the app. Each one listens on TCP port **47800** by
   default (change it under Settings; the port is shown at the top of the
   Chat tab together with your local IP addresses).
2. Allow the port through the firewall on each machine. On Windows the first
   launch usually prompts for this.
3. On one machine choose **Add peer by IP**, enter the other machine's address
   and an optional name. As soon as the two connect, the peer shows up on both
   sides with a green dot.
4. Type to chat; use the paperclip to send files. Received files land in
   `Downloads/Trakzen Conecta` (configurable).

Traffic is plain TCP on your LAN. Do not expose the port to the internet;
encryption is on the roadmap.

### Testing chat on one machine

Two instances can share a machine if the second one gets its own data
directory and port:

```sh
TRAKZEN_CONECTA_DATA_DIR=/tmp/conecta-b TRAKZEN_CONECTA_CHAT_PORT=47801 pnpm tauri dev
```

Then add `127.0.0.1` port `47801` as a peer from the first instance.

## How it is put together

```
src/                    React + TypeScript UI (Vite, Tailwind)
  features/mail         folders, message list, reading pane, composer
  features/chat         peers, conversation, transfers
  features/settings
  lib/ipc.ts            typed wrappers around Tauri commands and events
src-tauri/src/
  db/                   SQLite connection + migrations (rusqlite, bundled)
  mail/
    mod.rs              MailProvider trait – the seam for new providers
    gmail/              OAuth (PKCE + loopback), REST client, sync
    store.rs            local message cache
    compose.rs          RFC 5322 building, reply/forward drafts
    sanitize.rs         HTML mail cleaning (ammonia)
  chat/
    protocol.rs         framed TCP wire format
    engine.rs           listener, connections, file transfers
    store.rs            peers + history
  secrets.rs            OS credential store access
  settings.rs           key/value settings in SQLite
```

Design notes worth knowing:

- **Local first.** Every list and body the UI shows comes from SQLite. The
  network only ever runs in background tasks that emit events (`mail://sync`,
  `chat://message`, …) which the stores react to.
- **Gmail sync** does one full pull of recent metadata (300 messages, fetched
  8 at a time) and records the mailbox `historyId`. From then on
  `history.list` returns just the changes. Bodies are fetched on first open
  and cached.
- **HTML mail** is sanitised in Rust and rendered inside a sandboxed iframe
  with remote images blocked until you ask for them. Links open in the system
  browser.
- **Chat wire format** is `kind:u8 | len:u32 | payload`. Control frames are
  JSON, file chunks are binary. A file transfer opens its own TCP connection
  so a large upload never delays messages.
- **Adding a provider** means implementing `MailProvider` (login, sync,
  fetch_body, send, flags, trash, archive) in a new module and registering it
  in `Providers::provider_for`. The UI and store do not change.

## Roadmap

- Encrypted chat sessions (Noise) and mDNS peer discovery
- Group chats
- Inline (`cid:`) images in HTML mail
- Second mail provider (IMAP/SMTP generic, then Outlook)
- Rebinding the chat port without a restart

Contributions and bug reports are welcome.

## License

[MIT](LICENSE)
