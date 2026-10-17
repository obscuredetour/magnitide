//! Metadata-source keys (the Google Books API key, the Metron token) in the
//! system keychain: the macOS Keychain, Windows Credential Manager, or the
//! Secret Service on Linux (GNOME Keyring, KWallet). One entry per source,
//! filed under the app's identifier with the source's id as the account.
//!
//! The keys used to live in the sources' settings file. On the first launch
//! with a keychain reachable, any key still in the file moves into the
//! keychain and the file is rewritten without it; from then on the file holds
//! only the rest of the settings. Where no keychain is reachable (a headless
//! or minimal Linux without a Secret Service) the keys stay in the settings
//! file, readable only by its owner, and Settings says so.
//!
//! The keychain is only touched off the main thread (commands reach it from
//! blocking tasks): on Linux the Secret Service client runs its own runtime,
//! and a locked keyring may ask the user to unlock it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use magnitide_sources::{Settings, SourceId};
use serde::Serialize;
use ts_rs::TS;

/// The keychain service every key is filed under; the account is the source's id.
pub const SERVICE: &str = "com.magnitide.app";

/// What the keychain is called on this platform, for a sentence.
const PLACE: &str = if cfg!(target_os = "macos") {
    "the macOS Keychain"
} else if cfg!(windows) {
    "Windows Credential Manager"
} else {
    "the system keychain (Secret Service)"
};

/// A place that keeps one secret per account name.
pub trait KeyStore: Send + Sync {
    /// The secret, or None when there isn't one.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    /// Nothing to delete is fine.
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// The system keychain, through the keyring crate.
struct Keychain;

impl Keychain {
    /// The platform's keychain, if one is reachable.
    fn open() -> Result<Box<dyn KeyStore>, String> {
        keyring::Entry::store_status().as_ref().map_err(|e| e.to_string())?;
        Ok(Box::new(Keychain))
    }

    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
    }
}

impl KeyStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match Self::entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        Self::entry(account)?.set_password(secret).map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Where the sources' keys are kept, as Settings says it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct KeyStorage {
    /// For a sentence: "the macOS Keychain", or "the settings file".
    pub place: String,
    /// Why they're in the settings file rather than the keychain, when they are.
    pub fallback: Option<String>,
}

/// The sources' settings file and, when one is reachable, the keychain the
/// keys are in. Opened on first use (off the main thread; see [`Secrets::warm`]).
pub struct Secrets {
    settings_path: PathBuf,
    inner: OnceLock<Inner>,
}

impl Secrets {
    pub fn new(settings_path: PathBuf) -> Self {
        Self { settings_path, inner: OnceLock::new() }
    }

    fn inner(&self) -> &Inner {
        self.inner.get_or_init(|| Inner::open(&self.settings_path, Keychain::open()))
    }

    /// Reach the keychain and move any keys out of the settings file now,
    /// rather than at the first search. Blocks: call from a thread of its own.
    pub fn warm(&self) {
        self.inner();
    }

    pub fn storage(&self) -> KeyStorage {
        self.inner().storage.clone()
    }

    /// The sources' settings: the file's, with the keys the keychain holds.
    pub fn settings(&self) -> Settings {
        self.inner().settings(&self.settings_path)
    }

    /// Give a source its key, or clear it (blank), where keys are kept;
    /// `settings` follows (save it afterwards). Nothing changes on an error.
    pub fn set_key(&self, settings: &mut Settings, id: SourceId, key: &str) -> Result<(), String> {
        self.inner().set_key(settings, id, key)
    }

    /// Write the settings file, without the keys the keychain holds.
    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        self.inner().save(&self.settings_path, settings)
    }
}

struct Inner {
    /// None: no keychain; the keys stay in the settings file.
    store: Option<Box<dyn KeyStore>>,
    storage: KeyStorage,
    /// The keys the keychain holds, read once; set and delete keep it current.
    cache: RwLock<BTreeMap<SourceId, String>>,
}

impl Inner {
    /// Open the keychain, read the keys it has and move the settings file's
    /// keys into it; or, with no keychain (`Err(why)`), leave the file as it is.
    fn open(settings_path: &Path, keychain: Result<Box<dyn KeyStore>, String>) -> Self {
        let store = match keychain {
            Ok(store) => store,
            Err(why) => {
                return Self {
                    store: None,
                    storage: KeyStorage {
                        place: "the settings file".to_owned(),
                        fallback: Some(format!("No system keychain was reachable ({why}).")),
                    },
                    cache: RwLock::new(BTreeMap::new()),
                };
            }
        };
        let mut cache = BTreeMap::new();
        for id in SourceId::ALL.into_iter().filter(|id| id.needs_key()) {
            match store.get(id.id()) {
                Ok(Some(key)) => {
                    cache.insert(id, key);
                }
                Ok(None) => {}
                Err(e) => eprintln!("reading the {} key from {PLACE}: {e}", id.name()),
            }
        }
        let mut settings = read_file(settings_path);
        let moved = migrate(&mut settings, &*store);
        if !moved.is_empty() {
            match write_file(settings_path, &settings) {
                Ok(()) => cache.extend(moved),
                // The keys are in both places; next launch tries the file again.
                Err(e) => eprintln!("writing {} without its keys: {e}", settings_path.display()),
            }
        }
        Self {
            store: Some(store),
            storage: KeyStorage { place: PLACE.to_owned(), fallback: None },
            cache: RwLock::new(cache),
        }
    }

    fn settings(&self, settings_path: &Path) -> Settings {
        let mut settings = read_file(settings_path);
        for (id, key) in self.cache.read().unwrap_or_else(|e| e.into_inner()).iter() {
            settings.set_key(*id, Some(key));
        }
        settings
    }

    fn set_key(&self, settings: &mut Settings, id: SourceId, key: &str) -> Result<(), String> {
        let key = key.trim();
        if let Some(store) = &self.store {
            let mut cache = self.cache.write().unwrap_or_else(|e| e.into_inner());
            if key.is_empty() {
                store.delete(id.id()).map_err(|e| format!("Couldn't remove the key from {PLACE}: {e}"))?;
                cache.remove(&id);
            } else {
                store.set(id.id(), key).map_err(|e| format!("Couldn't save the key to {PLACE}: {e}"))?;
                cache.insert(id, key.to_owned());
            }
        }
        settings.set_key(id, Some(key));
        Ok(())
    }

    fn save(&self, settings_path: &Path, settings: &Settings) -> Result<(), String> {
        let mut on_disk = settings.clone();
        for id in self.cache.read().unwrap_or_else(|e| e.into_inner()).keys() {
            on_disk.set_key(*id, None);
        }
        write_file(settings_path, &on_disk)
    }
}

/// Move every key in `settings` into `store` (keyed by the source's id) and
/// take it out of `settings`; the moved keys. A key the store won't take
/// stays where it was.
fn migrate(settings: &mut Settings, store: &dyn KeyStore) -> Vec<(SourceId, String)> {
    let keys: Vec<(SourceId, String)> = settings.keys().map(|(id, k)| (id, k.to_owned())).collect();
    let mut moved = Vec::new();
    for (id, key) in keys {
        match store.set(id.id(), &key) {
            Ok(()) => {
                settings.set_key(id, None);
                moved.push((id, key));
            }
            Err(e) => eprintln!("moving the {} key into {PLACE}: {e}", id.name()),
        }
    }
    moved
}

/// The settings file's contents; defaults when there's no file or it's unreadable.
fn read_file(path: &Path) -> Settings {
    fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Write the settings file, for this user's eyes only (it may still hold
/// keys where there's no keychain).
fn write_file(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// A keychain in memory; `broken` refuses every write.
    #[derive(Default)]
    struct FakeStore {
        secrets: Mutex<BTreeMap<String, String>>,
        broken: bool,
    }

    impl FakeStore {
        fn with(secrets: &[(&str, &str)]) -> Self {
            let secrets = secrets.iter().map(|(a, s)| (a.to_string(), s.to_string())).collect();
            Self { secrets: Mutex::new(secrets), broken: false }
        }

        fn all(&self) -> BTreeMap<String, String> {
            self.secrets.lock().unwrap().clone()
        }
    }

    impl KeyStore for FakeStore {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.secrets.lock().unwrap().get(account).cloned())
        }

        fn set(&self, account: &str, secret: &str) -> Result<(), String> {
            if self.broken {
                return Err("the keychain is locked".into());
            }
            self.secrets.lock().unwrap().insert(account.to_owned(), secret.to_owned());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), String> {
            if self.broken {
                return Err("the keychain is locked".into());
            }
            self.secrets.lock().unwrap().remove(account);
            Ok(())
        }
    }

    fn file_with(dir: &Path, json: &str) -> PathBuf {
        let path = dir.join("metadata-sources.json");
        fs::write(&path, json).unwrap();
        path
    }

    fn on_disk(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn migrate_moves_keys_out_of_the_settings() {
        let mut settings = Settings {
            disabled: vec![SourceId::OpenLibrary],
            google_key: Some("g-key".into()),
            metron_key: Some("m-token".into()),
        };
        let store = FakeStore::default();
        let moved = migrate(&mut settings, &store);
        assert_eq!(moved, [(SourceId::Google, "g-key".to_owned()), (SourceId::Metron, "m-token".to_owned())]);
        assert_eq!(
            store.all(),
            BTreeMap::from([("google".into(), "g-key".into()), ("metron".into(), "m-token".into())])
        );
        assert_eq!(settings.keys().count(), 0, "{settings:?}");
        assert_eq!(settings.disabled, [SourceId::OpenLibrary], "the rest is untouched");
        assert!(migrate(&mut settings, &store).is_empty(), "nothing left to move");
    }

    #[test]
    fn a_key_the_store_refuses_stays_in_the_settings() {
        let mut settings = Settings { google_key: Some("g-key".into()), ..Default::default() };
        let store = FakeStore { broken: true, ..Default::default() };
        assert!(migrate(&mut settings, &store).is_empty());
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        assert!(store.all().is_empty());
    }

    #[test]
    fn opening_migrates_the_file_and_rewrites_it_without_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), r#"{"disabled":["openLibrary"],"googleKey":"g-key","metronKey":"m-token"}"#);
        let store = FakeStore::with(&[("metron", "old-token")]);
        let inner = Inner::open(&path, Ok(Box::new(store)));
        assert_eq!(inner.storage, KeyStorage { place: PLACE.to_owned(), fallback: None });
        let written = on_disk(&path);
        assert_eq!(written["disabled"], serde_json::json!(["openLibrary"]));
        assert!(written["googleKey"].is_null() && written["metronKey"].is_null(), "{written}");
        // The settings carry the keys from the keychain; the file's token replaced the old one.
        let settings = inner.settings(&path);
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        assert_eq!(settings.key(SourceId::Metron), Some("m-token"));
        assert_eq!(settings.disabled, [SourceId::OpenLibrary]);
    }

    #[test]
    fn opening_reads_keys_the_keychain_already_has() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), r#"{"disabled":[]}"#);
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        let store = FakeStore::with(&[("google", "g-key")]);
        let inner = Inner::open(&path, Ok(Box::new(store)));
        assert_eq!(inner.settings(&path).key(SourceId::Google), Some("g-key"));
        assert!(!inner.settings(&path).enabled(SourceId::Metron));
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before, "nothing to move: not rewritten");
    }

    #[test]
    fn opening_without_a_file_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("metadata-sources.json");
        let inner = Inner::open(&path, Ok(Box::new(FakeStore::default())));
        assert_eq!(inner.settings(&path), Settings::default());
        assert!(!path.exists());
    }

    #[test]
    fn without_a_keychain_keys_stay_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), r#"{"disabled":[],"googleKey":"g-key"}"#);
        let inner = Inner::open(&path, Err("no Secret Service on the session bus".into()));
        assert_eq!(inner.storage.place, "the settings file");
        assert!(inner.storage.fallback.as_deref().unwrap().contains("no Secret Service on the session bus"));
        let mut settings = inner.settings(&path);
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        inner.set_key(&mut settings, SourceId::Metron, " m-token ").unwrap();
        inner.save(&path, &settings).unwrap();
        let written = on_disk(&path);
        assert_eq!(written["googleKey"], "g-key");
        assert_eq!(written["metronKey"], "m-token");
    }

    #[test]
    fn setting_and_clearing_keys_goes_to_the_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), r#"{"disabled":[]}"#);
        let inner = Inner::open(&path, Ok(Box::new(FakeStore::default())));
        let mut settings = inner.settings(&path);
        inner.set_key(&mut settings, SourceId::Google, " g-key ").unwrap();
        assert_eq!(settings.key(SourceId::Google), Some("g-key"), "the settings follow at once");
        inner.save(&path, &settings).unwrap();
        assert!(on_disk(&path)["googleKey"].is_null(), "the file never gets the key");
        assert_eq!(inner.settings(&path).key(SourceId::Google), Some("g-key"));

        inner.set_key(&mut settings, SourceId::Google, "").unwrap();
        assert_eq!(settings.key(SourceId::Google), None);
        assert_eq!(inner.settings(&path).key(SourceId::Google), None);
    }

    #[test]
    fn a_failed_keychain_write_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), r#"{"disabled":[]}"#);
        let inner = Inner::open(&path, Ok(Box::new(FakeStore { broken: true, ..Default::default() })));
        let mut settings = inner.settings(&path);
        let err = inner.set_key(&mut settings, SourceId::Google, "g-key").unwrap_err();
        assert!(err.contains("the keychain is locked"), "{err}");
        assert_eq!(settings.key(SourceId::Google), None);
        assert_eq!(inner.settings(&path).key(SourceId::Google), None);
    }

    #[test]
    fn a_key_left_in_the_file_is_still_used_and_still_written() {
        // A migration that failed (the keychain refused the key) leaves the
        // key in the file: the source keeps working, and saving the other
        // settings doesn't lose it.
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), r#"{"disabled":[],"googleKey":"g-key"}"#);
        let inner = Inner::open(&path, Ok(Box::new(FakeStore { broken: true, ..Default::default() })));
        let mut settings = inner.settings(&path);
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        settings.disabled.push(SourceId::Metron);
        inner.save(&path, &settings).unwrap();
        assert_eq!(on_disk(&path)["googleKey"], "g-key");
    }

    /// The real keychain, once round: for a developer checking a platform
    /// (`cargo test -p magnitide -- --ignored keychain`); CI never runs it.
    #[test]
    #[ignore = "uses the system keychain"]
    fn keychain_round_trip() {
        let store = Keychain::open().expect("a keychain");
        let account = "magnitide-test";
        store.set(account, "s3cret").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("s3cret"));
        store.delete(account).unwrap();
        assert_eq!(store.get(account).unwrap(), None);
        store.delete(account).expect("deleting what isn't there is fine");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_the_owners_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metadata-sources.json");
        write_file(&path, &Settings::default()).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
