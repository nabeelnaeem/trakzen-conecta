//! Which server folders play which part (Sent, Trash, ...) and how a
//! message in one of them is named locally. Kept free of network code so
//! it can be tested against fixture LIST responses.

use std::collections::{HashMap, HashSet};

use crate::mail::types::RemoteLabel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Role {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
}

impl Role {
    /// Folders a sync reads, in order, with how many of the newest
    /// messages each contributes.
    pub(super) const SYNCED: [(Role, usize); 6] = [
        (Role::Inbox, 200),
        (Role::Sent, 100),
        (Role::Drafts, 100),
        (Role::Junk, 100),
        (Role::Trash, 100),
        (Role::Archive, 100),
    ];

    /// The system label messages in this folder carry. Archived mail has
    /// none, which is what puts it under All mail / Archive.
    pub(super) fn label(self) -> Option<&'static str> {
        match self {
            Role::Inbox => Some("INBOX"),
            Role::Sent => Some("SENT"),
            Role::Drafts => Some("DRAFT"),
            Role::Trash => Some("TRASH"),
            Role::Junk => Some("SPAM"),
            Role::Archive => None,
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Role::Inbox => "Inbox",
            Role::Sent => "Sent",
            Role::Drafts => "Drafts",
            Role::Trash => "Trash",
            Role::Junk => "Spam",
            Role::Archive => "Archive",
        }
    }

    /// Name used when the folder has to be created.
    pub(super) fn default_name(self) -> &'static str {
        match self {
            Role::Inbox => "INBOX",
            Role::Sent => "Sent",
            Role::Drafts => "Drafts",
            Role::Trash => "Trash",
            Role::Junk => "Junk",
            Role::Archive => "Archive",
        }
    }

    /// RFC 6154 SPECIAL-USE attribute. `\All` (Gmail's All Mail) and
    /// `\Flagged` are views over other folders and deliberately unmapped.
    fn from_special_use(attr: &str) -> Option<Role> {
        match attr.to_ascii_lowercase().as_str() {
            "\\sent" => Some(Role::Sent),
            "\\drafts" => Some(Role::Drafts),
            "\\trash" => Some(Role::Trash),
            "\\junk" => Some(Role::Junk),
            "\\archive" => Some(Role::Archive),
            _ => None,
        }
    }

    /// Common names for servers that do not advertise SPECIAL-USE, most
    /// likely first.
    fn known_names(self) -> &'static [&'static str] {
        match self {
            Role::Inbox => &["INBOX"],
            Role::Sent => &["Sent", "Sent Items", "Sent Messages", "Sent Mail", "INBOX.Sent", "[Gmail]/Sent Mail"],
            Role::Drafts => &["Drafts", "INBOX.Drafts"],
            Role::Trash => &["Trash", "Deleted Items", "Deleted Messages", "INBOX.Trash"],
            Role::Junk => &["Junk", "Junk Mail", "Junk E-mail", "Spam", "INBOX.Junk"],
            Role::Archive => &["Archive", "Archives", "INBOX.Archive"],
        }
    }
}

/// One LIST entry, detached from the imap crate's borrowed types.
#[derive(Debug, Clone)]
pub(super) struct ListedFolder {
    pub name: String,
    pub delimiter: Option<String>,
    pub attributes: Vec<String>,
}

impl ListedFolder {
    fn selectable(&self) -> bool {
        !self
            .attributes
            .iter()
            .any(|a| a.eq_ignore_ascii_case("\\Noselect") || a.eq_ignore_ascii_case("\\NonExistent"))
    }

    /// Compares against a well-known name level by level, so "INBOX.Sent"
    /// also matches a server that writes it "INBOX/Sent".
    fn is_named(&self, known: &str) -> bool {
        if self.name.eq_ignore_ascii_case(known) {
            return true;
        }
        let theirs: Vec<&str> = match self.delimiter.as_deref() {
            Some(d) if !d.is_empty() => self.name.split(d).collect(),
            _ => vec![self.name.as_str()],
        };
        let ours: Vec<&str> = known.split(['.', '/']).collect();
        theirs.len() == ours.len() && theirs.iter().zip(&ours).all(|(a, b)| a.eq_ignore_ascii_case(b))
    }
}

/// Server folder name per role, for one account.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct FolderMap {
    roles: HashMap<Role, String>,
    pub delimiter: Option<String>,
}

impl FolderMap {
    pub(super) fn from_list(folders: &[ListedFolder]) -> Self {
        let usable: Vec<&ListedFolder> = folders.iter().filter(|f| f.selectable()).collect();
        let mut map = FolderMap {
            roles: HashMap::new(),
            delimiter: folders.iter().find_map(|f| f.delimiter.clone()),
        };
        let inbox = usable
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case("INBOX"))
            .map_or_else(|| "INBOX".to_string(), |f| f.name.clone());
        map.roles.insert(Role::Inbox, inbox);
        // Flags win over names: a server may keep an unused "Sent" next to
        // the "Sent Items" it actually flags.
        for f in &usable {
            for a in &f.attributes {
                if let Some(role) = Role::from_special_use(a) {
                    map.roles.entry(role).or_insert_with(|| f.name.clone());
                }
            }
        }
        for role in [Role::Sent, Role::Drafts, Role::Trash, Role::Junk, Role::Archive] {
            if map.roles.contains_key(&role) {
                continue;
            }
            let taken: HashSet<&String> = map.roles.values().collect();
            let found = role
                .known_names()
                .iter()
                .find_map(|known| usable.iter().find(|f| f.is_named(known) && !taken.contains(&f.name)));
            if let Some(f) = found {
                map.roles.insert(role, f.name.clone());
            }
        }
        map
    }

    pub(super) fn get(&self, role: Role) -> Option<&str> {
        self.roles.get(&role).map(String::as_str)
    }

    pub(super) fn set(&mut self, role: Role, folder: String) {
        self.roles.insert(role, folder);
    }

    pub(super) fn role_of(&self, folder: &str) -> Option<Role> {
        if folder.eq_ignore_ascii_case("INBOX") {
            return Some(Role::Inbox);
        }
        self.roles.iter().find(|(_, f)| f.as_str() == folder).map(|(r, _)| *r)
    }

    /// System labels for the folders this account has, so the sidebar can
    /// count them.
    pub(super) fn labels(&self) -> Vec<RemoteLabel> {
        Role::SYNCED
            .iter()
            .filter(|(role, _)| self.roles.contains_key(role))
            .filter_map(|(role, _)| {
                role.label().map(|id| RemoteLabel {
                    remote_id: id.into(),
                    name: role.display_name().into(),
                    kind: "system".into(),
                    bg_color: None,
                    fg_color: None,
                    visible: true,
                })
            })
            .collect()
    }
}

/// Separates the parts of a remote id. Folder names may hold spaces, dots
/// and slashes but never control characters.
const ID_SEP: char = '\u{1f}';

/// A message's place on the server. `validity` is `None` for ids written
/// before folders were tracked, which were all INBOX UIDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MsgRef {
    pub folder: String,
    pub validity: Option<u32>,
    pub uid: u32,
}

impl MsgRef {
    pub(super) fn parse(remote_id: &str) -> Option<MsgRef> {
        let mut parts = remote_id.rsplitn(3, ID_SEP);
        let uid = parts.next()?.parse().ok()?;
        match (parts.next(), parts.next()) {
            (None, None) => Some(MsgRef {
                folder: "INBOX".into(),
                validity: None,
                uid,
            }),
            (Some(validity), Some(folder)) if !folder.is_empty() => Some(MsgRef {
                folder: folder.into(),
                validity: Some(validity.parse().ok()?),
                uid,
            }),
            _ => None,
        }
    }
}

pub(super) fn remote_id(folder: &str, validity: u32, uid: u32) -> String {
    format!("{folder}{ID_SEP}{validity}{ID_SEP}{uid}")
}

/// Compact UID set for a command: "3:5,9".
pub(super) fn uid_set(uids: &[u32]) -> String {
    let mut v = uids.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let start = v[i];
        while i + 1 < v.len() && v[i + 1] == v[i] + 1 {
            i += 1;
        }
        out.push(if v[i] == start { start.to_string() } else { format!("{start}:{}", v[i]) });
        i += 1;
    }
    out.join(",")
}

/// What a sync saw in one folder.
#[derive(Debug, Clone)]
pub(super) struct FolderScan {
    pub folder: String,
    pub validity: u32,
    pub uids: HashSet<u32>,
    /// First UID the server had not handed out yet when the folder was read.
    pub uid_next: u32,
}

/// Cached ids to drop after a sync: rows from before ids named a folder,
/// and rows for messages a scanned folder no longer holds (expunged, moved
/// away, or from an older UIDVALIDITY). Rows numbered past the scan come
/// from a move made while it ran and are kept.
pub(super) fn stale_ids(known: &[String], scans: &[FolderScan]) -> Vec<String> {
    known
        .iter()
        .filter(|id| match MsgRef::parse(id) {
            None => false,
            Some(r) => match r.validity {
                None => true,
                Some(v) => scans.iter().find(|s| s.folder == r.folder).is_some_and(|s| {
                    v != s.validity || (r.uid < s.uid_next && !s.uids.contains(&r.uid))
                }),
            },
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, delimiter: &str, attributes: &[&str]) -> ListedFolder {
        ListedFolder {
            name: name.into(),
            delimiter: Some(delimiter.into()),
            attributes: attributes.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn special_use_flags_pick_outlook_style_folders() {
        // Stalwart with Outlook-style names; the unflagged "Sent" must lose
        // to the folder the server marks \Sent.
        let map = FolderMap::from_list(&[
            folder("INBOX", "/", &["\\HasNoChildren"]),
            folder("Sent", "/", &["\\HasNoChildren"]),
            folder("Sent Items", "/", &["\\HasNoChildren", "\\Sent"]),
            folder("Deleted Items", "/", &["\\HasNoChildren", "\\Trash"]),
            folder("Junk Mail", "/", &["\\HasNoChildren", "\\Junk"]),
            folder("Drafts", "/", &["\\HasNoChildren", "\\Drafts"]),
            folder("Projects/2026", "/", &["\\HasNoChildren"]),
        ]);
        assert_eq!(map.get(Role::Inbox), Some("INBOX"));
        assert_eq!(map.get(Role::Sent), Some("Sent Items"));
        assert_eq!(map.get(Role::Trash), Some("Deleted Items"));
        assert_eq!(map.get(Role::Junk), Some("Junk Mail"));
        assert_eq!(map.get(Role::Drafts), Some("Drafts"));
        assert_eq!(map.get(Role::Archive), None);
        assert_eq!(map.role_of("Deleted Items"), Some(Role::Trash));
        assert_eq!(map.role_of("Projects/2026"), None);
        let labels: Vec<String> = map.labels().into_iter().map(|l| l.remote_id).collect();
        assert_eq!(labels, vec!["INBOX", "SENT", "DRAFT", "SPAM", "TRASH"]);
    }

    #[test]
    fn names_are_used_when_the_server_has_no_flags() {
        let map = FolderMap::from_list(&[
            folder("INBOX", ".", &[]),
            folder("INBOX.Sent", ".", &[]),
            folder("INBOX.Trash", ".", &[]),
            folder("INBOX.spam", ".", &[]),
            folder("INBOX.Drafts", ".", &[]),
            folder("Archives", ".", &[]),
        ]);
        assert_eq!(map.get(Role::Sent), Some("INBOX.Sent"));
        assert_eq!(map.get(Role::Trash), Some("INBOX.Trash"));
        assert_eq!(map.get(Role::Drafts), Some("INBOX.Drafts"));
        assert_eq!(map.get(Role::Archive), Some("Archives"));
        // "INBOX.spam" is not a known name; only top-level "Spam" is.
        assert_eq!(map.get(Role::Junk), None);
        assert_eq!(map.delimiter.as_deref(), Some("."));

        let slashed = FolderMap::from_list(&[
            folder("Inbox", "/", &[]),
            folder("INBOX/Sent", "/", &[]),
            folder("deleted messages", "/", &[]),
            folder("Junk E-mail", "/", &[]),
        ]);
        assert_eq!(slashed.get(Role::Inbox), Some("Inbox"));
        assert_eq!(slashed.get(Role::Sent), Some("INBOX/Sent"));
        assert_eq!(slashed.get(Role::Trash), Some("deleted messages"));
        assert_eq!(slashed.get(Role::Junk), Some("Junk E-mail"));
    }

    #[test]
    fn gmail_all_mail_and_noselect_parents_are_ignored() {
        let map = FolderMap::from_list(&[
            folder("INBOX", "/", &[]),
            folder("[Gmail]", "/", &["\\HasChildren", "\\Noselect"]),
            folder("[Gmail]/All Mail", "/", &["\\All", "\\HasNoChildren"]),
            folder("[Gmail]/Sent Mail", "/", &["\\HasNoChildren"]),
            folder("[Gmail]/Spam", "/", &["\\HasNoChildren", "\\Junk"]),
            folder("[Gmail]/Bin", "/", &["\\HasNoChildren", "\\Trash"]),
        ]);
        assert_eq!(map.get(Role::Sent), Some("[Gmail]/Sent Mail"));
        assert_eq!(map.get(Role::Junk), Some("[Gmail]/Spam"));
        assert_eq!(map.get(Role::Trash), Some("[Gmail]/Bin"));
        assert_eq!(map.get(Role::Archive), None);
    }

    #[test]
    fn remote_ids_round_trip_awkward_folder_names() {
        for name in ["INBOX", "Sent Items", "INBOX.Archive", "[Gmail]/Sent Mail", "a/b.c d"] {
            let id = remote_id(name, 1_700_000_001, 42);
            assert_eq!(
                MsgRef::parse(&id),
                Some(MsgRef { folder: name.into(), validity: Some(1_700_000_001), uid: 42 })
            );
        }
        assert_eq!(MsgRef::parse("17"), Some(MsgRef { folder: "INBOX".into(), validity: None, uid: 17 }));
        assert_eq!(MsgRef::parse("x\u{1f}17"), None);
        assert_eq!(MsgRef::parse("\u{1f}1\u{1f}17"), None);
        assert_eq!(MsgRef::parse("INBOX\u{1f}v\u{1f}17"), None);
        assert_eq!(MsgRef::parse("nope"), None);
    }

    #[test]
    fn uid_sets_collapse_runs() {
        assert_eq!(uid_set(&[9, 3, 4, 5, 5, 11, 12]), "3:5,9,11:12");
        assert_eq!(uid_set(&[7]), "7");
        assert_eq!(uid_set(&[]), "");
    }

    #[test]
    fn stale_rows_are_legacy_gone_or_from_an_old_uidvalidity() {
        let scan = FolderScan {
            folder: "INBOX".into(),
            validity: 5,
            uids: [1, 2, 3].into_iter().collect(),
            uid_next: 10,
        };
        let known: Vec<String> = vec![
            "2".into(),
            remote_id("INBOX", 5, 2),
            remote_id("INBOX", 5, 4),
            remote_id("INBOX", 4, 2),
            remote_id("INBOX", 5, 12),
            remote_id("Archive", 1, 99),
        ];
        let stale = stale_ids(&known, &[scan]);
        assert_eq!(stale, vec!["2".to_string(), remote_id("INBOX", 5, 4), remote_id("INBOX", 4, 2)]);
    }
}
