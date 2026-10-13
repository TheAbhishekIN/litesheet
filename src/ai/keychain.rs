//! API key storage in the system keychain (macOS Keychain, Windows
//! Credential Manager, Secret Service on Linux).

const KEYRING_SERVICE: &str = "LiteSheet";
const KEYRING_USER: &str = "ai-api-key";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

pub fn load_key() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|k| !k.is_empty())
}

pub fn store_key(key: &str) -> Result<(), String> {
    entry()?.set_password(key.trim()).map_err(|e| e.to_string())
}

pub fn delete_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
