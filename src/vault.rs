//! Vault lifecycle: initialize, inspect, unlock, change passphrase, recover (Spec 002, Task 002B).

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

use data_encoding::BASE32_NOPAD;
use zeroize::Zeroizing;

use crate::{
    crypto::{self, CryptoError, ID_LEN, KDF_ARGON2ID, KdfParams, SALT_LEN, SlotBinding},
    paths,
    secret::{KEY_LEN, Key, MIN_PASSPHRASE_CHARS, Passphrase},
    store::{
        self, FORMAT_VERSION, Header, KDF_NONE, SLOT_PASSPHRASE, SLOT_RECOVERY, Slot, Store,
        StoreError,
    },
};

const MANIFEST_VERSION: u8 = 1;
const MANIFEST_HEADER_LEN: usize = 1 + 8 + 4;
const RECOVERY_SLOT_PARAMS: KdfParams = KdfParams {
    m_kib: 0,
    t: 0,
    p: 0,
};

#[derive(Debug)]
pub enum VaultError {
    NotInitialized(PathBuf),
    AlreadyExists(PathBuf),
    NoDataDir,
    /// Wrong passphrase or recovery key, or a damaged key slot. Indistinguishable by design.
    Auth,
    Integrity(String),
    NoRecoveryKey,
    Input(String),
    Terminal(io::Error),
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Random,
}

impl VaultError {
    /// Exit codes from Spec 002 FR-005.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Auth => 3,
            Self::Integrity(_) => 4,
            Self::NotInitialized(_) => 5,
            _ => 1,
        }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized(path) => write!(
                f,
                "no vault at {}; create one with: maxonkey vault init",
                path.display()
            ),
            Self::AlreadyExists(path) => write!(
                f,
                "a vault already exists at {}; it was not changed",
                path.display()
            ),
            Self::NoDataDir => {
                f.write_str("could not determine the data directory; pass --vault <PATH>")
            }
            Self::Auth => f.write_str("wrong passphrase or recovery key, or the vault is damaged"),
            Self::Integrity(detail) => write!(f, "vault integrity check failed: {detail}"),
            Self::NoRecoveryKey => f.write_str("this vault has no recovery key"),
            Self::Input(message) => f.write_str(message),
            Self::Terminal(error) => write!(
                f,
                "cannot use the terminal ({error}); MaxonKey reads passphrases only from an interactive terminal"
            ),
            Self::Io(error) => write!(f, "file error: {error}"),
            Self::Sqlite(error) => write!(f, "database error: {error}"),
            Self::Random => f.write_str("the operating system random generator failed"),
        }
    }
}

impl From<StoreError> for VaultError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Integrity(detail) => Self::Integrity(detail),
            StoreError::Sqlite(rusqlite::Error::SqliteFailure(code, _))
                if matches!(
                    code.code,
                    rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt
                ) =>
            {
                Self::Integrity("the file is not a valid vault database".into())
            }
            StoreError::Sqlite(error) => Self::Sqlite(error),
        }
    }
}

impl From<io::Error> for VaultError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<CryptoError> for VaultError {
    fn from(error: CryptoError) -> Self {
        match error {
            CryptoError::Auth => Self::Auth,
            CryptoError::Format => Self::Integrity("unrecognized encrypted data format".into()),
            CryptoError::Kdf => {
                Self::Integrity("key derivation parameters are out of range".into())
            }
            CryptoError::Random => Self::Random,
        }
    }
}

pub type Result<T> = std::result::Result<T, VaultError>;

/// Resolves `--vault` or the OS-standard location.
pub fn resolve_path(explicit: Option<&Path>) -> Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path.to_path_buf()),
        None => paths::default_vault_path().ok_or(VaultError::NoDataDir),
    }
}

pub fn check_new_passphrase(passphrase: &Passphrase) -> Result<()> {
    if passphrase.char_count() < MIN_PASSPHRASE_CHARS {
        return Err(VaultError::Input(format!(
            "the passphrase must have at least {MIN_PASSPHRASE_CHARS} characters; a phrase of four or more random words is recommended"
        )));
    }
    Ok(())
}

// ----- Recovery key display format -----

pub fn new_recovery_key() -> Result<Key> {
    Ok(crypto::random_key()?)
}

/// Grouped Base32 (RFC 4648, no padding): 13 groups of 4 characters.
pub fn format_recovery_key(key: &Key) -> Zeroizing<String> {
    let encoded = Zeroizing::new(BASE32_NOPAD.encode(key.expose()));
    let mut grouped = Zeroizing::new(String::with_capacity(encoded.len() + encoded.len() / 4));
    for (index, ch) in encoded.chars().enumerate() {
        if index > 0 && index % 4 == 0 {
            grouped.push('-');
        }
        grouped.push(ch);
    }
    grouped
}

/// Accepts the displayed form with any case, spaces, or dashes.
pub fn parse_recovery_key(input: &str) -> Result<Key> {
    let mut compact = Zeroizing::new(String::with_capacity(input.len()));
    compact.extend(
        input
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase()),
    );
    let invalid = || VaultError::Input("that is not a valid recovery key".into());
    if BASE32_NOPAD
        .decode_len(compact.len())
        .map_err(|_| invalid())?
        != KEY_LEN
    {
        return Err(invalid());
    }
    let mut key = Key::zeroed();
    BASE32_NOPAD
        .decode_mut(compact.as_bytes(), key.expose_mut())
        .map_err(|_| invalid())?;
    Ok(key)
}

// ----- Manifest (record list; always empty in Task 002B) -----

fn encode_manifest(generation: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(MANIFEST_HEADER_LEN);
    out.push(MANIFEST_VERSION);
    out.extend_from_slice(&generation.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

fn decode_manifest(plain: &[u8]) -> Result<(u64, u32)> {
    let bad = |detail: &str| VaultError::Integrity(format!("manifest {detail}"));
    if plain.len() < MANIFEST_HEADER_LEN || plain[0] != MANIFEST_VERSION {
        return Err(bad("has an unknown format"));
    }
    let generation = u64::from_le_bytes(plain[1..9].try_into().expect("eight bytes"));
    let count = u32::from_le_bytes(plain[9..13].try_into().expect("four bytes"));
    let expected = (count as usize)
        .checked_mul(ID_LEN + 32)
        .and_then(|n| n.checked_add(MANIFEST_HEADER_LEN))
        .ok_or_else(|| bad("is malformed"))?;
    if plain.len() != expected {
        return Err(bad("is malformed"));
    }
    Ok((generation, count))
}

fn seal_manifest(vk: &Key, header: &Header, generation: u64) -> Result<Vec<u8>> {
    let padded = crypto::pad(&encode_manifest(generation));
    Ok(crypto::seal(
        vk,
        &crypto::manifest_aad(&header.vault_id),
        &padded,
    )?)
}

// ----- Key slots -----

fn binding<'a>(header: &'a Header, slot: &'a Slot) -> SlotBinding<'a> {
    SlotBinding {
        format_version: header.format_version,
        vault_id: &header.vault_id,
        slot_id: slot.slot_id,
        kdf_id: slot.kdf_id,
        params: slot.params,
        salt: &slot.salt,
    }
}

fn wrap_slot(header: &Header, mut slot: Slot, kek: &Key, vk: &Key) -> Result<Slot> {
    let aad = crypto::slot_aad(&binding(header, &slot));
    slot.wrapped_vk = crypto::seal(kek, &aad, vk.expose())?;
    Ok(slot)
}

fn passphrase_slot(
    header: &Header,
    passphrase: &Passphrase,
    params: KdfParams,
    vk: &Key,
) -> Result<Slot> {
    // Unit tests use cheaper parameters; the CLI always passes `KdfParams::DEFAULT`.
    if !params.valid_for_create() && !cfg!(test) {
        return Err(VaultError::Input(
            "key derivation parameters are below the required minimum".into(),
        ));
    }
    let salt: [u8; SALT_LEN] = crypto::random_array()?;
    let kek = crypto::derive_kek(passphrase, &salt, params)?;
    let slot = Slot {
        slot_id: SLOT_PASSPHRASE,
        kdf_id: KDF_ARGON2ID,
        params,
        salt,
        wrapped_vk: Vec::new(),
    };
    wrap_slot(header, slot, &kek, vk)
}

fn recovery_slot(header: &Header, recovery: &Key, vk: &Key) -> Result<Slot> {
    let slot = Slot {
        slot_id: SLOT_RECOVERY,
        kdf_id: KDF_NONE,
        params: RECOVERY_SLOT_PARAMS,
        salt: crypto::random_array()?,
        wrapped_vk: Vec::new(),
    };
    wrap_slot(header, slot, recovery, vk)
}

fn unwrap_slot(header: &Header, slot: &Slot, kek: &Key) -> Result<Key> {
    let aad = crypto::slot_aad(&binding(header, slot));
    let plain = crypto::open(kek, &aad, &slot.wrapped_vk)?;
    let bytes: &[u8; KEY_LEN] = plain
        .expose()
        .try_into()
        .map_err(|_| VaultError::Integrity("key slot has the wrong length".into()))?;
    let mut vk = Key::zeroed();
    vk.expose_mut().copy_from_slice(bytes);
    Ok(vk)
}

// ----- Lifecycle -----

/// Creates a new vault. Never overwrites an existing file (SR-015).
pub fn init(
    path: &Path,
    passphrase: &Passphrase,
    recovery: Option<&Key>,
    params: KdfParams,
) -> Result<()> {
    if path.symlink_metadata().is_ok() {
        return Err(VaultError::AlreadyExists(path.to_path_buf()));
    }
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    };
    paths::create_private_dir(&dir)?;

    let header = Header {
        format_version: FORMAT_VERSION,
        vault_id: crypto::random_array()?,
    };
    let vk = crypto::random_key()?;
    let mut slots = vec![passphrase_slot(&header, passphrase, params, &vk)?];
    if let Some(recovery) = recovery {
        slots.push(recovery_slot(&header, recovery, &vk)?);
    }
    let manifest = seal_manifest(&vk, &header, 0)?;
    drop(vk);

    // Build the vault in a private temporary file beside the target, then move it into
    // place only if nothing exists there. The temporary file is removed on any error.
    let temp = tempfile::Builder::new()
        .prefix(".vault-init-")
        .suffix(".tmp")
        .tempfile_in(&dir)?
        .into_temp_path();
    store::create(&temp, &header, &slots, &manifest)?;
    temp.persist_noclobber(path).map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            VaultError::AlreadyExists(path.to_path_buf())
        } else {
            VaultError::Io(error.error)
        }
    })?;
    paths::sync_dir(&dir)?;
    Ok(())
}

/// What can be learned without the passphrase.
#[derive(Debug)]
pub enum VaultStatus {
    NotInitialized,
    Initialized {
        format_version: u32,
        params: Option<KdfParams>,
        recovery_key: bool,
        warnings: Vec<String>,
    },
}

pub fn status(path: &Path) -> Result<VaultStatus> {
    if path.symlink_metadata().is_err() {
        return Ok(VaultStatus::NotInitialized);
    }
    let store = Store::open(path, false)?;
    let header = store.header()?;
    let params = store.slot(SLOT_PASSPHRASE)?.map(|slot| slot.params);
    Ok(VaultStatus::Initialized {
        format_version: header.format_version,
        params,
        recovery_key: store.slot(SLOT_RECOVERY)?.is_some(),
        warnings: paths::permission_warnings(path),
    })
}

pub enum Credential<'a> {
    Passphrase(&'a Passphrase),
    RecoveryKey(&'a Key),
}

/// An unlocked vault. Dropping it wipes the vault key (lock).
pub struct Unlocked {
    store: Store,
    header: Header,
    vk: Key,
    generation: u64,
}

impl fmt::Debug for Unlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Unlocked([REDACTED])")
    }
}

/// Opens the vault and verifies the header, key slot, and manifest before any write (SR-013).
pub fn unlock(path: &Path, credential: Credential<'_>) -> Result<Unlocked> {
    if path.symlink_metadata().is_err() {
        return Err(VaultError::NotInitialized(path.to_path_buf()));
    }
    let store = Store::open(path, true)?;
    let header = store.header()?;
    let vk = match credential {
        Credential::Passphrase(passphrase) => {
            let slot = store
                .slot(SLOT_PASSPHRASE)?
                .ok_or_else(|| VaultError::Integrity("passphrase key slot is missing".into()))?;
            if slot.kdf_id != KDF_ARGON2ID || !slot.params.within_open() {
                return Err(VaultError::Integrity(
                    "key derivation parameters are out of range".into(),
                ));
            }
            let kek = crypto::derive_kek(passphrase, &slot.salt, slot.params)?;
            unwrap_slot(&header, &slot, &kek)?
        }
        Credential::RecoveryKey(recovery) => {
            let slot = store
                .slot(SLOT_RECOVERY)?
                .ok_or(VaultError::NoRecoveryKey)?;
            if slot.kdf_id != KDF_NONE {
                return Err(VaultError::Integrity(
                    "recovery key slot is malformed".into(),
                ));
            }
            unwrap_slot(&header, &slot, recovery)?
        }
    };
    // The vault key is now known to be correct, so any failure below is tampering.
    let manifest = crypto::open(
        &vk,
        &crypto::manifest_aad(&header.vault_id),
        &store.manifest()?,
    )
    .map_err(|_| VaultError::Integrity("the manifest failed authentication".into()))?;
    let (generation, count) = decode_manifest(
        crypto::unpad(manifest.expose())
            .map_err(|_| VaultError::Integrity("the manifest is malformed".into()))?,
    )?;
    if count != 0 || store.record_count()? != 0 {
        return Err(VaultError::Integrity(
            "the vault contains records this version of MaxonKey cannot read".into(),
        ));
    }
    Ok(Unlocked {
        store,
        header,
        vk,
        generation,
    })
}

impl Unlocked {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Re-wraps the vault key under a new passphrase. Records are not re-encrypted.
    pub fn change_passphrase(&mut self, new: &Passphrase, params: KdfParams) -> Result<()> {
        let slot = passphrase_slot(&self.header, new, params, &self.vk)?;
        self.store.replace_slot(&self.header, &slot)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::tests::TEST_PARAMS;
    use std::fs;

    const PASS: &str = "MK-CANARY-pass-7c1e9b2f";

    fn passphrase(text: &str) -> Passphrase {
        Passphrase::from_input(Zeroizing::new(text.to_owned()))
    }

    fn new_vault(recovery: Option<&Key>) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("vault.sqlite3");
        init(&path, &passphrase(PASS), recovery, TEST_PARAMS).unwrap();
        (dir, path)
    }

    fn sql(path: &Path, statement: &str) {
        rusqlite::Connection::open(path)
            .unwrap()
            .execute_batch(statement)
            .unwrap();
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn init_then_unlock_with_passphrase_and_recovery_key() {
        let recovery = new_recovery_key().unwrap();
        let (_dir, path) = new_vault(Some(&recovery));
        let unlocked = unlock(&path, Credential::Passphrase(&passphrase(PASS))).unwrap();
        assert_eq!(unlocked.generation(), 0);
        let parsed = parse_recovery_key(&format_recovery_key(&recovery).to_lowercase()).unwrap();
        unlock(&path, Credential::RecoveryKey(&parsed)).unwrap();
        match status(&path).unwrap() {
            VaultStatus::Initialized {
                format_version,
                params,
                recovery_key,
                ..
            } => {
                assert_eq!(format_version, 1);
                assert_eq!(params, Some(TEST_PARAMS));
                assert!(recovery_key);
            }
            other => panic!("{other:?}"),
        }
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("vault.sqlite3")]);
    }

    /// ST-02 (002B part): no passphrase or recovery key material in the vault file.
    #[test]
    fn vault_file_contains_no_credentials() {
        let recovery = new_recovery_key().unwrap();
        let (_dir, path) = new_vault(Some(&recovery));
        let bytes = fs::read(&path).unwrap();
        let utf16: Vec<u8> = PASS.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let hex: String = PASS.bytes().map(|b| format!("{b:02x}")).collect();
        let b32 = BASE32_NOPAD.encode(PASS.as_bytes());
        for needle in [PASS.as_bytes(), &utf16, hex.as_bytes(), b32.as_bytes()] {
            assert!(!contains(&bytes, needle));
        }
        assert!(!contains(&bytes, recovery.expose()));
        assert!(!contains(&bytes, format_recovery_key(&recovery).as_bytes()));
    }

    /// ST-06.
    #[test]
    fn wrong_credentials_fail_generically_and_leave_file_unchanged() {
        let recovery = new_recovery_key().unwrap();
        let (_dir, path) = new_vault(Some(&recovery));
        let before = fs::read(&path).unwrap();
        let error = unlock(
            &path,
            Credential::Passphrase(&passphrase("wrong passphrase!")),
        )
        .unwrap_err();
        assert!(matches!(error, VaultError::Auth));
        assert_eq!(error.exit_code(), 3);
        let other = new_recovery_key().unwrap();
        assert!(matches!(
            unlock(&path, Credential::RecoveryKey(&other)).unwrap_err(),
            VaultError::Auth
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    /// ST-10.
    #[test]
    fn init_never_overwrites() {
        let (_dir, path) = new_vault(None);
        let before = fs::read(&path).unwrap();
        let error = init(&path, &passphrase("another passphrase"), None, TEST_PARAMS).unwrap_err();
        assert!(matches!(error, VaultError::AlreadyExists(_)));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn passphrase_rules() {
        assert!(check_new_passphrase(&passphrase("eleven char")).is_err());
        assert!(check_new_passphrase(&passphrase("twelve chars")).is_ok());
        // 12 characters, more bytes: counted in characters, not bytes.
        assert!(check_new_passphrase(&passphrase("ééééééééééé")).is_err());
        assert!(check_new_passphrase(&passphrase("éééééééééééé")).is_ok());
    }

    /// ST-17.
    #[test]
    fn change_passphrase_rewraps_only_the_passphrase_slot() {
        let recovery = new_recovery_key().unwrap();
        let (_dir, path) = new_vault(Some(&recovery));
        let manifest_before = Store::open(&path, false).unwrap().manifest().unwrap();
        let mut unlocked = unlock(&path, Credential::Passphrase(&passphrase(PASS))).unwrap();
        unlocked
            .change_passphrase(&passphrase("brand new passphrase"), TEST_PARAMS)
            .unwrap();
        drop(unlocked);
        assert!(matches!(
            unlock(&path, Credential::Passphrase(&passphrase(PASS))).unwrap_err(),
            VaultError::Auth
        ));
        unlock(
            &path,
            Credential::Passphrase(&passphrase("brand new passphrase")),
        )
        .unwrap();
        unlock(&path, Credential::RecoveryKey(&recovery)).unwrap();
        assert_eq!(
            Store::open(&path, false).unwrap().manifest().unwrap(),
            manifest_before
        );
    }

    #[test]
    fn recover_without_recovery_slot() {
        let (_dir, path) = new_vault(None);
        let key = new_recovery_key().unwrap();
        assert!(matches!(
            unlock(&path, Credential::RecoveryKey(&key)).unwrap_err(),
            VaultError::NoRecoveryKey
        ));
    }

    /// ST-19.
    #[test]
    fn nfc_and_nfd_passphrases_open_the_same_vault() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.sqlite3");
        init(
            &path,
            &passphrase("caf\u{e9} au lait daily"),
            None,
            TEST_PARAMS,
        )
        .unwrap();
        unlock(
            &path,
            Credential::Passphrase(&passphrase("cafe\u{301} au lait daily")),
        )
        .unwrap();
    }

    /// ST-03 (002B part): every byte of every authenticated field is protected.
    #[test]
    fn tampering_with_header_slots_or_manifest_is_detected() {
        let recovery = new_recovery_key().unwrap();
        let (_dir, path) = new_vault(Some(&recovery));
        let pristine = fs::read(&path).unwrap();
        tamper_sweep(&path, &pristine, &recovery);
    }

    fn flip_byte(path: &Path, table: &str, column: &str, filter: &str, index: usize) {
        let conn = rusqlite::Connection::open(path).unwrap();
        let mut value: Vec<u8> = conn
            .query_row(
                &format!("SELECT {column} FROM {table} WHERE {filter}"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        value[index] ^= 0x01;
        conn.execute(
            &format!("UPDATE {table} SET {column} = ?1 WHERE {filter}"),
            [value],
        )
        .unwrap();
    }

    fn tamper_sweep(path: &Path, pristine: &[u8], recovery: &Key) {
        // (table, column, row filter, length, passphrase still opens, recovery key still opens)
        let targets = [
            ("vault_header", "vault_id", "1", 16, false, false),
            ("key_slots", "salt", "slot_id = 1", 16, false, true),
            ("key_slots", "wrapped_vk", "slot_id = 1", 73, false, true),
            ("key_slots", "wrapped_vk", "slot_id = 2", 73, true, false),
            ("manifest", "blob", "1", 105, false, false),
        ];
        for (table, column, filter, len, pass_ok, key_ok) in targets {
            for index in 0..len {
                fs::write(path, pristine).unwrap();
                flip_byte(path, table, column, filter, index);
                let place = format!("{table}.{column}[{index}] ({filter})");
                for (result, expected_ok) in [
                    (
                        unlock(path, Credential::Passphrase(&passphrase(PASS))),
                        pass_ok,
                    ),
                    (unlock(path, Credential::RecoveryKey(recovery)), key_ok),
                ] {
                    match result {
                        Ok(_) => assert!(expected_ok, "{place} was accepted"),
                        Err(error) => {
                            assert!(!expected_ok, "{place}: {error}");
                            assert!(matches!(error.exit_code(), 3 | 4), "{place}: {error}");
                        }
                    }
                }
            }
        }
        // Integer header fields are bound too.
        for statement in [
            "UPDATE key_slots SET kdf_t = kdf_t + 1 WHERE slot_id = 1",
            "UPDATE key_slots SET kdf_m = kdf_m * 2 WHERE slot_id = 1",
            "UPDATE key_slots SET kdf_p = 2 WHERE slot_id = 1",
            "UPDATE key_slots SET kdf_id = 0 WHERE slot_id = 1",
            "UPDATE vault_header SET format_version = 2",
            "DELETE FROM manifest",
            "DELETE FROM key_slots WHERE slot_id = 1",
            "INSERT INTO records VALUES (zeroblob(16), x'00', NULL)",
        ] {
            fs::write(path, pristine).unwrap();
            sql(path, statement);
            let error =
                unlock(path, Credential::Passphrase(&passphrase(PASS))).expect_err(statement);
            assert!(matches!(error.exit_code(), 3 | 4), "{statement}: {error}");
        }
        fs::write(path, pristine).unwrap();
    }

    /// ST-04 (002B part): a key slot or manifest copied from another vault is rejected.
    #[test]
    fn blobs_from_another_vault_are_rejected() {
        let (_a_dir, a) = new_vault(None);
        let (_b_dir, b) = new_vault(None);
        let b_store = Store::open(&b, false).unwrap();
        let b_manifest = b_store.manifest().unwrap();
        let b_slot = b_store.slot(SLOT_PASSPHRASE).unwrap().unwrap();
        drop(b_store);
        let pristine = fs::read(&a).unwrap();
        let conn = rusqlite::Connection::open(&a).unwrap();
        conn.execute("UPDATE manifest SET blob = ?1", [&b_manifest])
            .unwrap();
        drop(conn);
        assert_eq!(
            unlock(&a, Credential::Passphrase(&passphrase(PASS)))
                .unwrap_err()
                .exit_code(),
            4
        );
        fs::write(&a, &pristine).unwrap();
        let conn = rusqlite::Connection::open(&a).unwrap();
        conn.execute(
            "UPDATE key_slots SET salt = ?1, wrapped_vk = ?2 WHERE slot_id = 1",
            (b_slot.salt.as_slice(), &b_slot.wrapped_vk),
        )
        .unwrap();
        drop(conn);
        assert_eq!(
            unlock(&a, Credential::Passphrase(&passphrase(PASS)))
                .unwrap_err()
                .exit_code(),
            3
        );
    }

    /// ST-07: a header asking for absurd KDF cost is refused before allocation.
    #[test]
    fn excessive_kdf_parameters_are_refused() {
        let (_dir, path) = new_vault(None);
        for statement in [
            "UPDATE key_slots SET kdf_m = 4194304 WHERE slot_id = 1",
            "UPDATE key_slots SET kdf_t = 100 WHERE slot_id = 1",
            "UPDATE key_slots SET kdf_p = 255 WHERE slot_id = 1",
        ] {
            let pristine = fs::read(&path).unwrap();
            sql(&path, statement);
            let error = unlock(&path, Credential::Passphrase(&passphrase(PASS))).unwrap_err();
            assert_eq!(error.exit_code(), 4, "{statement}");
            fs::write(&path, &pristine).unwrap();
        }
    }

    /// ST-20.
    #[test]
    fn unknown_versions_and_foreign_files_are_refused() {
        let (_dir, path) = new_vault(None);
        let pristine = fs::read(&path).unwrap();
        for statement in ["PRAGMA user_version = 2", "PRAGMA application_id = 7"] {
            sql(&path, statement);
            let before = fs::read(&path).unwrap();
            let error = unlock(&path, Credential::Passphrase(&passphrase(PASS))).unwrap_err();
            assert_eq!(error.exit_code(), 4, "{statement}");
            assert!(status(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
            fs::write(&path, &pristine).unwrap();
        }
        fs::write(&path, b"plain text, not a database at all").unwrap();
        assert_eq!(
            unlock(&path, Credential::Passphrase(&passphrase(PASS)))
                .unwrap_err()
                .exit_code(),
            4
        );
        assert_eq!(status(&path).unwrap_err().exit_code(), 4);
    }

    #[test]
    fn missing_vault_is_reported_as_not_initialized() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.sqlite3");
        assert!(matches!(
            status(&path).unwrap(),
            VaultStatus::NotInitialized
        ));
        let error = unlock(&path, Credential::Passphrase(&passphrase(PASS))).unwrap_err();
        assert_eq!(error.exit_code(), 5);
        assert!(!path.exists());
    }

    #[test]
    fn recovery_key_format() {
        let key = new_recovery_key().unwrap();
        let shown = format_recovery_key(&key);
        assert_eq!(shown.len(), 52 + 12);
        assert_eq!(shown.split('-').count(), 13);
        let spaced = shown.replace('-', " ");
        assert_eq!(parse_recovery_key(&spaced).unwrap().expose(), key.expose());
        for bad in ["", "ABCD", &shown[..shown.len() - 1], "1111-1111"] {
            assert!(parse_recovery_key(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn errors_never_contain_credentials() {
        let (_dir, path) = new_vault(None);
        let error = unlock(
            &path,
            Credential::Passphrase(&passphrase("MK-CANARY-wrong-1234")),
        )
        .unwrap_err();
        let text = format!("{error} {error:?}");
        assert!(!text.contains("MK-CANARY"));
        assert!(!text.contains(PASS));
    }
}
