//! Zero-configuration peer discovery over mDNS/DNS-SD. Every instance
//! advertises `_trakzen-conecta._tcp` with its peer id and display name and
//! browses for others; the UI lists what it finds as "nearby" so nobody has
//! to type an IP on a normal LAN. Manual IPs remain the fallback for
//! networks that block multicast.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::now_ms;

pub const SERVICE_TYPE: &str = "_trakzen-conecta._tcp.local.";
pub const EVENT_NEARBY: &str = "chat://nearby";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Nearby {
    pub peer_id: String,
    pub display_name: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub last_seen: i64,
}

pub struct Discovery {
    daemon: ServiceDaemon,
    found: Mutex<HashMap<String, Nearby>>,
    own_fullname: Mutex<Option<String>>,
}

impl Discovery {
    pub fn start(app: AppHandle, peer_id: String, display_name: String, port: u16) -> Option<Arc<Self>> {
        let daemon = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(%e, "mDNS unavailable; nearby peers disabled");
                return None;
            }
        };
        let this = Arc::new(Self {
            daemon,
            found: Mutex::new(HashMap::new()),
            own_fullname: Mutex::new(None),
        });
        this.advertise(&peer_id, &display_name, port);

        let receiver = match this.daemon.browse(SERVICE_TYPE) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(%e, "mDNS browse failed");
                return Some(this);
            }
        };
        let me = this.clone();
        let own_id = peer_id.clone();
        // mdns-sd is synchronous; a plain thread is the simplest home for it.
        std::thread::Builder::new()
            .name("mdns-browse".into())
            .spawn(move || {
                while let Ok(ev) = receiver.recv() {
                    match ev {
                        ServiceEvent::ServiceResolved(info) => {
                            let props = info.get_properties();
                            let Some(id) = props.get_property_val_str("id") else { continue };
                            if id == own_id {
                                continue;
                            }
                            let name = props
                                .get_property_val_str("name")
                                .unwrap_or("Unknown")
                                .to_string();
                            let mut addresses: Vec<String> = info
                                .get_addresses()
                                .iter()
                                .filter(|ip| matches!(ip, IpAddr::V4(v) if !v.is_loopback() && !v.is_link_local()))
                                .map(|ip| ip.to_string())
                                .collect();
                            addresses.sort();
                            if addresses.is_empty() {
                                continue;
                            }
                            me.found.lock().unwrap().insert(
                                id.to_string(),
                                Nearby {
                                    peer_id: id.to_string(),
                                    display_name: name,
                                    addresses,
                                    port: info.get_port(),
                                    last_seen: now_ms(),
                                },
                            );
                            me.emit(&app);
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            // Instance names are the peer id; drop it from the list.
                            let id = fullname.split('.').next().unwrap_or("").to_string();
                            me.found.lock().unwrap().remove(&id);
                            me.emit(&app);
                        }
                        _ => {}
                    }
                }
            })
            .ok();
        Some(this)
    }

    fn advertise(&self, peer_id: &str, display_name: &str, port: u16) {
        let host = format!("{}.local.", &peer_id[..peer_id.len().min(12)]);
        let props = [("id", peer_id), ("name", display_name)];
        match ServiceInfo::new(SERVICE_TYPE, peer_id, &host, "", port, &props[..]) {
            Ok(info) => {
                let info = info.enable_addr_auto();
                let fullname = info.get_fullname().to_string();
                if let Err(e) = self.daemon.register(info) {
                    tracing::warn!(%e, "mDNS register failed");
                } else {
                    *self.own_fullname.lock().unwrap() = Some(fullname);
                }
            }
            Err(e) => tracing::warn!(%e, "mDNS service info invalid"),
        }
    }

    /// Re-announces with a new display name.
    pub fn rename(&self, peer_id: &str, display_name: &str, port: u16) {
        if let Some(full) = self.own_fullname.lock().unwrap().take() {
            let _ = self.daemon.unregister(&full);
        }
        self.advertise(peer_id, display_name, port);
    }

    pub fn list(&self) -> Vec<Nearby> {
        let mut v: Vec<Nearby> = self.found.lock().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
        v
    }

    pub fn addresses_for(&self, peer_id: &str) -> Vec<(String, u16)> {
        self.found
            .lock()
            .unwrap()
            .get(peer_id)
            .map(|n| n.addresses.iter().map(|a| (a.clone(), n.port)).collect())
            .unwrap_or_default()
    }

    fn emit(&self, app: &AppHandle) {
        let _ = app.emit(EVENT_NEARBY, self.list());
    }
}
