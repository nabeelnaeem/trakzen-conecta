# Privacy Policy

Trakzen Conecta is a desktop application that runs entirely on your own
computer. There is no Trakzen Conecta server and no account with us.

## What the app accesses

- **Gmail.** When you connect a Google account the app requests the
  `gmail.modify` scope so it can read your messages, send replies, change
  labels (read/unread, starred, archive) and move messages to Trash. It
  cannot permanently delete mail.
- **Local network.** The chat feature listens on a TCP port you choose and
  exchanges messages and files directly with computers you add by IP
  address.

## Where your data goes

- Mail metadata, message bodies and attachments you open are cached in a
  SQLite database on your computer so the app can show them instantly.
- OAuth refresh tokens are stored in your operating system's credential
  store (Windows Credential Manager, macOS Keychain, or Secret Service on
  Linux).
- Chat history and received files are stored on your computer.
- Nothing is sent to the developer or to any third party. The only remote
  services the app talks to are Google's APIs (for Gmail) and the peers
  you add yourself (for chat).

## Google API Services User Data Policy

Trakzen Conecta's use of information received from Google APIs adheres to
the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy),
including the Limited Use requirements. Google user data is used only to
provide the mail features you see in the app, is never sold, and is never
used for advertising.

## Removing your data

Remove the account inside the app to revoke its token, or revoke access at
https://myaccount.google.com/permissions. Uninstalling the app and deleting
its data folder removes the local cache.

## Contact

Questions: open an issue at https://github.com/nabeelnaeem/trakzen-conecta.
