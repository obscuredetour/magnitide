//! Secrets in the system keychain: the metadata sources' keys (the Google
//! Books API key, the Metron token) and the library server's password go
//! to the macOS Keychain, Windows Credential Manager, or the Secret Service
//! on Linux (GNOME Keyring, KWallet). One entry each, filed under the app's
//! identifier with an account name of its own (a source's id, or
//! `server-password`).
//!
//! They used to live in their settings files. On the first launch with a
//! keychain reachable, any secret still in a file moves into the keychain
//! and the file is rewritten without it; from then on the files hold only
//! the rest of their settings. Where no keychain is reachable (a headless
//! or minimal Linux without a Secret Service) the secrets stay in their
//! settings files, readable only by their owner, and Settings says so.
//!
//! The keychain is only touched off the main thread (commands reach it from
//! blocking tasks): on Linux the Secret Service client runs its own runtime,
//! and a locked keyring may ask the user to unlock it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use magnitide_sources::SourceId;
use serde::Serialize;
use serde::de::DeserializeOwned;
use ts_rs::TS;

/// The keychain service every secret is filed under.
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

/// A settings file (JSON) with secrets in it, which the keychain takes over.
pub trait SecretFile: Serialize + DeserializeOwned + Default + Clone {
    /// The account names its secrets are filed under.
    fn accounts() -> Vec<&'static str>;
    /// Its secrets, by account.
    fn secrets(&self) -> Vec<(&'static str, String)>;
    /// Give it a secret (trimmed; blank or None clears it). An account that
    /// isn't one of its own is ignored.
    fn set_secret(&mut self, account: &str, secret: Option<&str>);
}

impl SecretFile for magnitide_sources::Settings {
    fn accounts() -> Vec<&'static str> {
        SourceId::ALL.into_iter().filter(|id| id.needs_key()).map(SourceId::id).collect()
    }

    fn secrets(&self) -> Vec<(&'static str, String)> {
        self.keys().map(|(id, key)| (id.id(), key.to_owned())).collect()
    }

    fn set_secret(&mut self, account: &str, secret: Option<&str>) {
        if let Some(id) = SourceId::ALL.into_iter().find(|id| id.id() == account) {
            self.set_key(id, secret);
        }
    }
}

/// Where secrets are kept, as Settings says it.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct KeyStorage {
    /// For a sentence: "the macOS Keychain", or "the settings file".
    pub place: String,
    /// Why they're in the settings file rather than the keychain, when they are.
    pub fallback: Option<String>,
}

/// A settings file registered with [`Secrets`]: its secrets are read from
/// the keychain at launch, and any still in the file move there.
pub struct Registered {
    path: PathBuf,
    accounts: Vec<&'static str>,
    migrate: fn(&Path, &dyn KeyStore) -> Vec<(String, String)>,
}

impl Registered {
    pub fn file<T: SecretFile>(path: PathBuf) -> Self {
        Self { path, accounts: T::accounts(), migrate: migrate_file::<T> }
    }
}

/// The keychain, when one is reachable, and the secrets it holds for the
/// registered settings files. Opened on first use (off the main thread; see
/// [`Secrets::warm`]).
pub struct Secrets {
    files: Vec<Registered>,
    inner: OnceLock<Inner>,
}

impl Secrets {
    pub fn new(files: Vec<Registered>) -> Self {
        Self { files, inner: OnceLock::new() }
    }

    fn inner(&self) -> &Inner {
        self.inner.get_or_init(|| Inner::open(&self.files, Keychain::open()))
    }

    /// Reach the keychain and move any secrets out of the settings files
    /// now, rather than at the first use. Blocks: call from a thread of its own.
    pub fn warm(&self) {
        self.inner();
    }

    pub fn storage(&self) -> KeyStorage {
        self.inner().storage.clone()
    }

    /// A settings file's contents, with the secrets the keychain holds.
    pub fn load<T: SecretFile>(&self, path: &Path) -> T {
        self.inner().load(path)
    }

    /// Give `settings` a secret, or clear it (blank), where secrets are
    /// kept; save the settings afterwards. Nothing changes on an error.
    pub fn set_secret<T: SecretFile>(&self, settings: &mut T, account: &str, secret: &str) -> Result<(), String> {
        self.inner().set_secret(settings, account, secret)
    }

    /// Write a settings file, without the secrets the keychain holds.
    pub fn save<T: SecretFile>(&self, path: &Path, settings: &T) -> Result<(), String> {
        self.inner().save(path, settings)
    }
}

struct Inner {
    /// None: no keychain; the secrets stay in their settings files.
    store: Option<Box<dyn KeyStore>>,
    storage: KeyStorage,
    /// The secrets the keychain holds, read once; set and delete keep it current.
    cache: RwLock<BTreeMap<String, String>>,
}

impl Inner {
    /// Open the keychain, read the files' secrets from it and move the
    /// files' own into it; or, with no keychain (`Err(why)`), leave the
    /// files as they are.
    fn open(files: &[Registered], keychain: Result<Box<dyn KeyStore>, String>) -> Self {
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
        for account in files.iter().flat_map(|f| f.accounts.iter().copied()) {
            match store.get(account) {
                Ok(Some(secret)) => {
                    cache.insert(account.to_owned(), secret);
                }
                Ok(None) => {}
                Err(e) => eprintln!("reading {account} from {PLACE}: {e}"),
            }
        }
        for file in files {
            cache.extend((file.migrate)(&file.path, &*store));
        }
        Self {
            store: Some(store),
            storage: KeyStorage { place: PLACE.to_owned(), fallback: None },
            cache: RwLock::new(cache),
        }
    }

    fn load<T: SecretFile>(&self, path: &Path) -> T {
        let mut settings: T = read_file(path);
        for (account, secret) in self.cache.read().unwrap_or_else(|e| e.into_inner()).iter() {
            settings.set_secret(account, Some(secret));
        }
        settings
    }

    fn set_secret<T: SecretFile>(&self, settings: &mut T, account: &str, secret: &str) -> Result<(), String> {
        let secret = secret.trim();
        if let Some(store) = &self.store {
            let mut cache = self.cache.write().unwrap_or_else(|e| e.into_inner());
            if secret.is_empty() {
                store.delete(account).map_err(|e| format!("Couldn't remove it from {PLACE}: {e}"))?;
                cache.remove(account);
            } else {
                store.set(account, secret).map_err(|e| format!("Couldn't save it to {PLACE}: {e}"))?;
                cache.insert(account.to_owned(), secret.to_owned());
            }
        }
        settings.set_secret(account, Some(secret));
        Ok(())
    }

    fn save<T: SecretFile>(&self, path: &Path, settings: &T) -> Result<(), String> {
        let mut on_disk = settings.clone();
        for account in self.cache.read().unwrap_or_else(|e| e.into_inner()).keys() {
            on_disk.set_secret(account, None);
        }
        write_file(path, &on_disk)
    }
}

/// Move the secrets in the file at `path` into `store` and rewrite the file
/// without them; the moved secrets. Nothing to move, nothing written.
fn migrate_file<T: SecretFile>(path: &Path, store: &dyn KeyStore) -> Vec<(String, String)> {
    let mut settings: T = read_file(path);
    let moved = migrate(&mut settings, store);
    if moved.is_empty() {
        return moved;
    }
    match write_file(path, &settings) {
        Ok(()) => moved,
        // The secrets are in both places; next launch tries the file again.
        Err(e) => {
            eprintln!("writing {} without its secrets: {e}", path.display());
            Vec::new()
        }
    }
}

/// Move every secret in `settings` into `store` and take it out of
/// `settings`; the moved secrets. One the store won't take stays where it was.
fn migrate<T: SecretFile>(settings: &mut T, store: &dyn KeyStore) -> Vec<(String, String)> {
    let mut moved = Vec::new();
    for (account, secret) in settings.secrets() {
        match store.set(account, &secret) {
            Ok(()) => {
                settings.set_secret(account, None);
                moved.push((account.to_owned(), secret));
            }
            Err(e) => eprintln!("moving {account} into {PLACE}: {e}"),
        }
    }
    moved
}

/// A settings file's contents; defaults when there's no file or it's unreadable.
fn read_file<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Write a settings file, for this user's eyes only (it may still hold
/// secrets where there's no keychain).
fn write_file<T: Serialize>(path: &Path, settings: &T) -> Result<(), String> {
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

    use magnitide_sources::Settings;

    use super::*;
    use crate::server::{self, SERVER_PASSWORD};

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

    fn file_with(dir: &Path, name: &str, json: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, json).unwrap();
        path
    }

    fn on_disk(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn sources(path: &Path) -> Registered {
        Registered::file::<Settings>(path.to_owned())
    }

    fn open(path: &Path, store: FakeStore) -> Inner {
        Inner::open(&[sources(path)], Ok(Box::new(store)))
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
        assert_eq!(moved, [("google".to_owned(), "g-key".to_owned()), ("metron".to_owned(), "m-token".to_owned())]);
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
        let path = file_with(
            dir.path(),
            "metadata-sources.json",
            r#"{"disabled":["openLibrary"],"googleKey":"g-key","metronKey":"m-token"}"#,
        );
        let inner = open(&path, FakeStore::with(&[("metron", "old-token")]));
        assert_eq!(inner.storage, KeyStorage { place: PLACE.to_owned(), fallback: None });
        let written = on_disk(&path);
        assert_eq!(written["disabled"], serde_json::json!(["openLibrary"]));
        assert!(written["googleKey"].is_null() && written["metronKey"].is_null(), "{written}");
        // The settings carry the keys from the keychain; the file's token replaced the old one.
        let settings: Settings = inner.load(&path);
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        assert_eq!(settings.key(SourceId::Metron), Some("m-token"));
        assert_eq!(settings.disabled, [SourceId::OpenLibrary]);
    }

    #[test]
    fn opening_reads_keys_the_keychain_already_has() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), "metadata-sources.json", r#"{"disabled":[]}"#);
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        let inner = open(&path, FakeStore::with(&[("google", "g-key")]));
        let settings: Settings = inner.load(&path);
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        assert!(!settings.enabled(SourceId::Metron));
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before, "nothing to move: not rewritten");
    }

    #[test]
    fn opening_without_a_file_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("metadata-sources.json");
        let inner = open(&path, FakeStore::default());
        assert_eq!(inner.load::<Settings>(&path), Settings::default());
        assert!(!path.exists());
    }

    #[test]
    fn without_a_keychain_keys_stay_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), "metadata-sources.json", r#"{"disabled":[],"googleKey":"g-key"}"#);
        let inner = Inner::open(&[sources(&path)], Err("no Secret Service on the session bus".into()));
        assert_eq!(inner.storage.place, "the settings file");
        assert!(inner.storage.fallback.as_deref().unwrap().contains("no Secret Service on the session bus"));
        let mut settings: Settings = inner.load(&path);
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        inner.set_secret(&mut settings, "metron", " m-token ").unwrap();
        inner.save(&path, &settings).unwrap();
        let written = on_disk(&path);
        assert_eq!(written["googleKey"], "g-key");
        assert_eq!(written["metronKey"], "m-token");
    }

    #[test]
    fn setting_and_clearing_keys_goes_to_the_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), "metadata-sources.json", r#"{"disabled":[]}"#);
        let inner = open(&path, FakeStore::default());
        let mut settings: Settings = inner.load(&path);
        inner.set_secret(&mut settings, "google", " g-key ").unwrap();
        assert_eq!(settings.key(SourceId::Google), Some("g-key"), "the settings follow at once");
        inner.save(&path, &settings).unwrap();
        assert!(on_disk(&path)["googleKey"].is_null(), "the file never gets the key");
        assert_eq!(inner.load::<Settings>(&path).key(SourceId::Google), Some("g-key"));

        inner.set_secret(&mut settings, "google", "").unwrap();
        assert_eq!(settings.key(SourceId::Google), None);
        assert_eq!(inner.load::<Settings>(&path).key(SourceId::Google), None);
    }

    #[test]
    fn a_failed_keychain_write_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), "metadata-sources.json", r#"{"disabled":[]}"#);
        let inner = open(&path, FakeStore { broken: true, ..Default::default() });
        let mut settings: Settings = inner.load(&path);
        let err = inner.set_secret(&mut settings, "google", "g-key").unwrap_err();
        assert!(err.contains("the keychain is locked"), "{err}");
        assert_eq!(settings.key(SourceId::Google), None);
        assert_eq!(inner.load::<Settings>(&path).key(SourceId::Google), None);
    }

    #[test]
    fn a_key_left_in_the_file_is_still_used_and_still_written() {
        // A migration that failed (the keychain refused the key) leaves the
        // key in the file: the source keeps working, and saving the other
        // settings doesn't lose it.
        let dir = tempfile::tempdir().unwrap();
        let path = file_with(dir.path(), "metadata-sources.json", r#"{"disabled":[],"googleKey":"g-key"}"#);
        let inner = open(&path, FakeStore { broken: true, ..Default::default() });
        let mut settings: Settings = inner.load(&path);
        assert_eq!(settings.key(SourceId::Google), Some("g-key"));
        settings.disabled.push(SourceId::Metron);
        inner.save(&path, &settings).unwrap();
        assert_eq!(on_disk(&path)["googleKey"], "g-key");
    }

    #[test]
    fn the_server_password_moves_with_the_keys() {
        let dir = tempfile::tempdir().unwrap();
        let keys = file_with(dir.path(), "metadata-sources.json", r#"{"disabled":[],"googleKey":"g-key"}"#);
        let server = file_with(dir.path(), "server.json", r#"{"enabled":true,"port":8181,"password":"hunter2"}"#);
        let store = FakeStore::default();
        let inner =
            Inner::open(&[sources(&keys), Registered::file::<server::Settings>(server.clone())], Ok(Box::new(store)));
        let written = on_disk(&server);
        assert_eq!(written["port"], 8181);
        assert!(written["password"].is_null(), "{written}");
        let settings: server::Settings = inner.load(&server);
        assert_eq!((settings.enabled, settings.port, settings.password.as_deref()), (true, 8181, Some("hunter2")));
        assert_eq!(inner.load::<Settings>(&keys).key(SourceId::Google), Some("g-key"));
        // Each file only ever sees its own secrets.
        assert!(on_disk(&keys).get("password").is_none());
        assert_eq!(inner.load::<Settings>(&keys).keys().count(), 1);

        let mut settings: server::Settings = inner.load(&server);
        inner.set_secret(&mut settings, SERVER_PASSWORD, "").unwrap();
        inner.save(&server, &settings).unwrap();
        assert_eq!(inner.load::<server::Settings>(&server).password, None);
        assert!(on_disk(&server)["password"].is_null());
    }

    #[test]
    fn without_a_keychain_the_server_password_stays_in_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = file_with(dir.path(), "server.json", r#"{"enabled":false,"port":8180,"password":"hunter2"}"#);
        let inner = Inner::open(&[Registered::file::<server::Settings>(server.clone())], Err("no keychain".into()));
        let mut settings: server::Settings = inner.load(&server);
        assert_eq!(settings.password.as_deref(), Some("hunter2"));
        inner.set_secret(&mut settings, SERVER_PASSWORD, "letmein").unwrap();
        inner.save(&server, &settings).unwrap();
        assert_eq!(on_disk(&server)["password"], "letmein");
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
