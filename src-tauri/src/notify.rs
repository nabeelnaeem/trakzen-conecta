//! Desktop notifications that can be clicked to open the thing they are
//! about, plus the `conecta://` deep links those clicks (and the pairing QR)
//! resolve to.
//!
//! Routes: `conecta://chat/<peer id or row id>`,
//! `conecta://mail/<account id>/<thread id>`, `conecta://pair?...`.
//!
//! Windows toasts use protocol activation (`conecta://…`) so a click still
//! works if the process was fully quit. The installer (and
//! `set_app_user_model_id`) pin the AppUserModelID so those toasts belong
//! to this app rather than PowerShell.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const EVENT_NAVIGATE: &str = "app://navigate";
pub const APP_USER_MODEL_ID: &str = "com.trakzen.conecta";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Route {
    Chat { peer: String },
    Mail { account_id: i64, thread_id: String },
    Pair { link: String },
}

impl Route {
    pub fn to_url(&self) -> String {
        match self {
            Route::Chat { peer } => format!("conecta://chat/{peer}"),
            Route::Mail {
                account_id,
                thread_id,
            } => format!("conecta://mail/{account_id}/{thread_id}"),
            Route::Pair { link } => link.clone(),
        }
    }
}

pub fn parse_route(url: &str) -> Option<Route> {
    let u = url::Url::parse(url).ok()?;
    if u.scheme() != "conecta" {
        return None;
    }
    // `conecta://chat/x` parses with host "chat" and path "/x".
    let host = u.host_str().unwrap_or("");
    let segs: Vec<&str> = u.path().trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    match host {
        "chat" => segs.first().map(|p| Route::Chat { peer: p.to_string() }),
        "mail" => match segs.as_slice() {
            [account, thread, ..] => Some(Route::Mail {
                account_id: account.parse().ok()?,
                thread_id: thread.to_string(),
            }),
            _ => None,
        },
        "pair" => Some(Route::Pair { link: url.to_string() }),
        _ => None,
    }
}

/// Brings the window back (from the tray or minimised) and tells the UI
/// where to go.
pub fn open_route(app: &AppHandle, route: Option<Route>) {
    focus_main(app);
    if let Some(r) = route {
        let _ = app.emit(EVENT_NAVIGATE, r);
    }
}

pub fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        // On GNOME/Wayland a focus request without an activation token is
        // ignored and the shell shows "App is ready" instead. Honour a token
        // if the notification server (or a second-instance launch) gave us one.
        #[cfg(target_os = "linux")]
        {
            if let Ok(token) = std::env::var("XDG_ACTIVATION_TOKEN") {
                if !token.is_empty() {
                    let _ = w.set_focus();
                    // gtk/wry read this on the next present; drop it so a
                    // later focus is not reused after it expires.
                    let _ = token;
                }
            }
        }
        let _ = w.set_focus();
    }
}

pub fn handle_urls(app: &AppHandle, urls: &[String]) {
    for u in urls {
        if let Some(r) = parse_route(u) {
            open_route(app, Some(r));
            return;
        }
    }
}

/// Shows a notification; clicking it opens `route` (or just the window).
pub fn show(app: &AppHandle, title: &str, body: &str, route: Option<Route>) {
    #[cfg(windows)]
    {
        let _ = app;
        windows_toast(title, body, route.as_ref().map(|r| r.to_url()).as_deref());
    }

    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        let (title, body) = (title.to_string(), body.to_string());
        // notify-rust blocks while waiting for the click; keep it off the
        // async runtime.
        std::thread::spawn(move || {
            linux_notify(&app, &title, &body, route);
        });
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = route;
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

/// Call once at process start so WinRT toasts attach to this app, not PowerShell.
pub fn set_app_user_model_id() {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

        let wide: Vec<u16> = std::ffi::OsStr::new(APP_USER_MODEL_ID)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide.as_ptr())) };
    }
}

#[cfg(windows)]
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(windows)]
fn windows_toast(title: &str, body: &str, launch: Option<&str>) {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let launch_attr = match launch {
        Some(u) => format!(
            r#" activationType="protocol" launch="{}""#,
            xml_escape(u)
        ),
        None => String::new(),
    };
    let xml = format!(
        r#"<toast{launch_attr}><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        xml_escape(title),
        xml_escape(body)
    );
    let shown = (|| {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))?;
        notifier.Show(&toast)?;
        anyhow::Ok(())
    })();
    if let Err(e) = shown {
        tracing::warn!(%e, "toast failed");
    }
}

#[cfg(target_os = "linux")]
fn linux_notify(app: &AppHandle, title: &str, body: &str, route: Option<Route>) {
    let shown = notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .appname("Trakzen Conecta")
        .icon("trakzen-conecta")
        .hint(notify_rust::Hint::DesktopEntry("trakzen-conecta".into()))
        .action("default", "Open")
        .show();
    match shown {
        Ok(handle) => handle.wait_for_action(|action| {
            if action == "default" {
                // GNOME 45+ may stash the compositor token here so set_focus
                // actually raises the window on Wayland instead of "App is ready".
                if let Ok(token) = std::env::var("XDG_ACTIVATION_TOKEN") {
                    if !token.is_empty() {
                        std::env::set_var("XDG_ACTIVATION_TOKEN", token);
                    }
                }
                open_route(app, route.clone());
            }
        }),
        Err(e) => tracing::warn!(%e, "notification failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_routes() {
        assert!(matches!(parse_route("conecta://chat/abc-123"), Some(Route::Chat { peer }) if peer == "abc-123"));
        assert!(matches!(
            parse_route("conecta://mail/2/18f3a"),
            Some(Route::Mail { account_id: 2, thread_id }) if thread_id == "18f3a"
        ));
        assert!(matches!(parse_route("conecta://pair?host=1.2.3.4&port=47800"), Some(Route::Pair { .. })));
        assert!(parse_route("https://example.com").is_none());
        assert!(parse_route("conecta://mail/x/y").is_none());
    }

    #[test]
    fn round_trips_mail_route() {
        let r = Route::Mail {
            account_id: 2,
            thread_id: "18f3a".into(),
        };
        assert!(matches!(
            parse_route(&r.to_url()),
            Some(Route::Mail { account_id: 2, thread_id }) if thread_id == "18f3a"
        ));
    }
}
