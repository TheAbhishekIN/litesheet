//! API key storage in the system keychain (macOS Keychain, Windows
//! Credential Manager, Secret Service on Linux), one entry per provider.

use super::Provider;

const KEYRING_SERVICE: &str = "LiteSheet";

fn entry(provider: Provider) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, provider.key_account()).map_err(|e| e.to_string())
}

pub fn load_key(provider: Provider) -> Option<String> {
    // Tests never read the real Keychain (macOS would prompt for access).
    if cfg!(test) {
        return None;
    }
    entry(provider)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.is_empty())
}

pub fn store_key(provider: Provider, key: &str) -> Result<(), String> {
    entry(provider)?
        .set_password(key.trim())
        .map_err(|e| e.to_string())
}

pub fn delete_key(provider: Provider) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
