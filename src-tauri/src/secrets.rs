//! OAuth refresh tokens live in the OS credential store (Windows Credential
//! Manager, macOS Keychain, Secret Service on Linux) rather than in SQLite so
//! a copied database file does not grant mailbox access.

use keyring::Entry;

use crate::error::Result;

const SERVICE: &str = "trakzen-conecta";

fn entry(key: &str) -> Result<Entry> {
    Ok(Entry::new(SERVICE, key)?)
}

pub fn get(key: &str) -> Result<Option<String>> {
    match entry(key)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn set(key: &str, value: &str) -> Result<()> {
    entry(key)?.set_password(value)?;
    Ok(())
}

pub fn delete(key: &str) -> Result<()> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
