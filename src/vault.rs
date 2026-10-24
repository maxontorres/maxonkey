//! Vault lifecycle and secret records (Spec 002, Tasks 002B and 002C): initialize, inspect,
//! unlock with full verification, change passphrase, recover, and set/get/list/delete secrets.

use std::{
    collections::{BTreeMap, HashSet},
    fmt, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use data_encoding::BASE32_NOPAD;
use zeroize::Zeroizing;

use crate::{
    crypto::{
        self, CryptoError, DIGEST_LEN, FIELD_META, FIELD_VALUE, ID_LEN, KDF_ARGON2ID, KdfParams,
        SALT_LEN, SlotBinding,
    },
    paths,
    secret::{KEY_LEN, Key, MIN_PASSPHRASE_CHARS, Passphrase, SecretBytes},
    store::{
        self, Change, FORMAT_VERSION, Header, KDF_NONE, RawRecord, SLOT_PASSPHRASE, SLOT_RECOVERY,
        Slot, Store, StoreError,
    },
};

const MANIFEST_VERSION: u8 = 1;
const MANIFEST_HEADER_LEN: usize = 1 + 8 + 4;
const MANIFEST_ENTRY_LEN: usize = ID_LEN + DIGEST_LEN;
const META_VERSION: u8 = 1;
/// FR-003: project, environment, and secret names are 1–128 characters.
pub const MAX_NAME_CHARS: usize = 128;
/// FR-003: secret values are 1 byte to 64 KiB.
pub const MAX_VALUE_LEN: usize = 64 * 1024;
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
    /// No secret at the requested address. Carries no names (SR-023).
    NotFound,
    Input(String),
    /// Invalid command input such as an over-long name (exit code 2).
    Usage(String),
    Terminal(io::Error),
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Random,
}

impl VaultError {
    /// Exit codes from Spec 002 FR-005.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
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
            Self::NotFound => f.write_str("no secret with that project, environment, and name"),
            Self::Input(message) | Self::Usage(message) => f.write_str(message),
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

// ----- Manifest: generation counter and the digest of every record (D7) -----

type Digest = [u8; DIGEST_LEN];
type RecordId = [u8; ID_LEN];

/// Layout: version (1) ‖ generation (8, LE) ‖ count (4, LE) ‖ count × (record id (16) ‖
/// SHA-256 (32)), with record ids in strictly ascending order.
fn encode_manifest(generation: u64, entries: &BTreeMap<RecordId, Digest>) -> Vec<u8> {
    let count = u32::try_from(entries.len()).expect("fewer than 2^32 records");
    let mut out = Vec::with_capacity(MANIFEST_HEADER_LEN + entries.len() * MANIFEST_ENTRY_LEN);
    out.push(MANIFEST_VERSION);
    out.extend_from_slice(&generation.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    for (id, digest) in entries {
        out.extend_from_slice(id);
        out.extend_from_slice(digest);
    }
    out
}

fn decode_manifest(plain: &[u8]) -> Result<(u64, BTreeMap<RecordId, Digest>)> {
    let bad = |detail: &str| VaultError::Integrity(format!("manifest {detail}"));
    if plain.len() < MANIFEST_HEADER_LEN || plain[0] != MANIFEST_VERSION {
        return Err(bad("has an unknown format"));
    }
    let generation = u64::from_le_bytes(plain[1..9].try_into().expect("eight bytes"));
    let count = u32::from_le_bytes(plain[9..13].try_into().expect("four bytes"));
    let expected = (count as usize)
        .checked_mul(MANIFEST_ENTRY_LEN)
        .and_then(|n| n.checked_add(MANIFEST_HEADER_LEN))
        .ok_or_else(|| bad("is malformed"))?;
    if plain.len() != expected {
        return Err(bad("is malformed"));
    }
    let mut entries = BTreeMap::new();
    let mut previous: Option<RecordId> = None;
    // The length check above guarantees there is no remainder.
    for chunk in plain[MANIFEST_HEADER_LEN..]
        .as_chunks::<MANIFEST_ENTRY_LEN>()
        .0
    {
        let id: RecordId = chunk[..ID_LEN].try_into().expect("sixteen bytes");
        if previous.is_some_and(|p| p >= id) {
            return Err(bad("is malformed"));
        }
        previous = Some(id);
        entries.insert(id, chunk[ID_LEN..].try_into().expect("thirty-two bytes"));
    }
    Ok((generation, entries))
}

fn seal_manifest(
    vk: &Key,
    header: &Header,
    generation: u64,
    entries: &BTreeMap<RecordId, Digest>,
) -> Result<Vec<u8>> {
    let padded = crypto::pad(&encode_manifest(generation, entries));
    Ok(crypto::seal(
        vk,
        &crypto::manifest_aad(&header.vault_id),
        &padded,
    )?)
}

/// SHA-256(meta_blob ‖ value_blob) over the stored ciphertext.
fn record_digest(meta_blob: &[u8], value_blob: Option<&[u8]>) -> Digest {
    crypto::sha256(&[meta_blob, value_blob.unwrap_or_default()])
}

// ----- Record metadata (inside meta_blob; SR-010) -----

/// Where a secret lives (FR-003). Names are case-sensitive and compared byte for byte.
#[derive(Clone, Copy)]
pub struct Address<'a> {
    pub project: &'a str,
    pub environment: &'a str,
    pub name: &'a str,
}

impl Address<'_> {
    fn check(&self) -> Result<()> {
        for (text, what) in [
            (self.project, "project"),
            (self.environment, "environment"),
            (self.name, "secret name"),
        ] {
            check_name(text, what)?;
        }
        Ok(())
    }
}

/// FR-003: 1–128 characters, no control characters. The message never repeats the input.
pub fn check_name(text: &str, what: &str) -> Result<()> {
    let count = text.chars().count();
    if count == 0 || count > MAX_NAME_CHARS || text.chars().any(char::is_control) {
        return Err(VaultError::Usage(format!(
            "the {what} must be 1 to {MAX_NAME_CHARS} characters with no control characters"
        )));
    }
    Ok(())
}

fn check_value(value: &[u8]) -> Result<()> {
    if value.is_empty() || value.len() > MAX_VALUE_LEN {
        return Err(VaultError::Usage(
            "the secret value must be between 1 byte and 64 KiB".into(),
        ));
    }
    Ok(())
}

/// Decrypted record metadata. Wiped on drop; never printed (SR-024, SR-025).
struct Meta {
    project: Zeroizing<String>,
    environment: Zeroizing<String>,
    name: Zeroizing<String>,
    notes: Zeroizing<String>,
    created: u64,
    updated: u64,
}

impl fmt::Debug for Meta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Meta([REDACTED])")
    }
}

impl Meta {
    fn matches(&self, address: &Address<'_>) -> bool {
        *self.project == address.project
            && *self.environment == address.environment
            && *self.name == address.name
    }

    /// Layout: version (1) ‖ created (8, LE) ‖ updated (8, LE) ‖ project, environment, name
    /// (each 2-byte LE length ‖ UTF-8) ‖ notes (4-byte LE length ‖ UTF-8). Times are Unix seconds.
    fn encode(&self) -> Zeroizing<Vec<u8>> {
        let strings = [&self.project, &self.environment, &self.name];
        let total =
            1 + 16 + strings.iter().map(|s| 2 + s.len()).sum::<usize>() + 4 + self.notes.len();
        let mut out = Zeroizing::new(Vec::with_capacity(total));
        out.push(META_VERSION);
        out.extend_from_slice(&self.created.to_le_bytes());
        out.extend_from_slice(&self.updated.to_le_bytes());
        for text in strings {
            let len = u16::try_from(text.len()).expect("names are at most 512 bytes");
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(text.as_bytes());
        }
        let len = u32::try_from(self.notes.len()).expect("notes are under 4 GiB");
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(self.notes.as_bytes());
        out
    }

    fn decode(plain: &[u8]) -> Result<Self> {
        let bad = || VaultError::Integrity("a record has malformed metadata".into());
        let mut reader = Reader(plain);
        if reader.take(1).ok_or_else(bad)?[0] != META_VERSION {
            return Err(bad());
        }
        let created = reader.u64().ok_or_else(bad)?;
        let updated = reader.u64().ok_or_else(bad)?;
        let meta = Self {
            project: reader.text(2).ok_or_else(bad)?,
            environment: reader.text(2).ok_or_else(bad)?,
            name: reader.text(2).ok_or_else(bad)?,
            notes: reader.text(4).ok_or_else(bad)?,
            created,
            updated,
        };
        if !reader.0.is_empty() {
            return Err(bad());
        }
        let address = Address {
            project: &meta.project,
            environment: &meta.environment,
            name: &meta.name,
        };
        address.check().map_err(|_| bad())?;
        Ok(meta)
    }
}

/// Reads fixed-layout fields from decrypted bytes; `None` means malformed.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// A UTF-8 string preceded by a 2- or 4-byte little-endian length.
    fn text(&mut self, len_bytes: usize) -> Option<Zeroizing<String>> {
        let raw = self.take(len_bytes)?;
        let len = match len_bytes {
            2 => u16::from_le_bytes(raw.try_into().ok()?) as usize,
            _ => u32::from_le_bytes(raw.try_into().ok()?) as usize,
        };
        let text = std::str::from_utf8(self.take(len)?).ok()?;
        Some(Zeroizing::new(text.to_owned()))
    }
}

/// One verified record: decrypted metadata plus its stored ciphertext.
struct Entry {
    id: RecordId,
    meta: Meta,
    meta_blob: Vec<u8>,
    value_blob: Vec<u8>,
}

impl Entry {
    fn digest(&self) -> Digest {
        record_digest(&self.meta_blob, Some(self.value_blob.as_slice()))
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
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
    let manifest = seal_manifest(&vk, &header, 0, &BTreeMap::new())?;
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

/// An unlocked vault. Dropping it wipes the vault key and the metadata index (lock).
pub struct Unlocked {
    store: Store,
    header: Header,
    vk: Key,
    generation: u64,
    /// The manifest ciphertext as verified at unlock; writes require it to be unchanged.
    manifest_blob: Vec<u8>,
    index: Vec<Entry>,
}

impl fmt::Debug for Unlocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Unlocked([REDACTED])")
    }
}

/// Opens the vault and verifies the header, key slot, manifest, and every record before
/// any read or write (SR-013).
pub fn unlock(path: &Path, credential: Credential<'_>) -> Result<Unlocked> {
    if path.symlink_metadata().is_err() {
        return Err(VaultError::NotInitialized(path.to_path_buf()));
    }
    let mut store = Store::open(path, true)?;
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
    // One read transaction, so another process's commit cannot land between the reads.
    let (manifest_blob, records) = store.snapshot()?;
    let manifest = crypto::open(&vk, &crypto::manifest_aad(&header.vault_id), &manifest_blob)
        .map_err(|_| VaultError::Integrity("the manifest failed authentication".into()))?;
    let (generation, expected) = decode_manifest(
        crypto::unpad(manifest.expose())
            .map_err(|_| VaultError::Integrity("the manifest is malformed".into()))?,
    )?;
    let index = verify_records(&vk, &header, &expected, records)?;
    Ok(Unlocked {
        store,
        header,
        vk,
        generation,
        manifest_blob,
        index,
    })
}

/// Checks that the stored records are exactly those in the manifest, each with a matching
/// digest and authentic metadata, and builds the in-memory index. Reports only counts.
fn verify_records(
    vk: &Key,
    header: &Header,
    expected: &BTreeMap<RecordId, Digest>,
    records: Vec<RawRecord>,
) -> Result<Vec<Entry>> {
    let mut index = Vec::with_capacity(records.len());
    let mut damaged = 0usize;
    let mut found = 0usize;
    for record in records {
        let Some(digest) = expected.get(&record.id) else {
            damaged += 1; // a row the manifest does not list
            continue;
        };
        found += 1;
        let Some(value_blob) = record.value else {
            damaged += 1;
            continue;
        };
        if record_digest(&record.meta, Some(value_blob.as_slice())) != *digest {
            damaged += 1;
            continue;
        }
        let aad = crypto::record_aad(&header.vault_id, &record.id, FIELD_META);
        let meta = crypto::open(vk, &aad, &record.meta)
            .ok()
            .and_then(|plain| Meta::decode(crypto::unpad(plain.expose()).ok()?).ok());
        match meta {
            Some(meta) => index.push(Entry {
                id: record.id,
                meta,
                meta_blob: record.meta,
                value_blob,
            }),
            None => damaged += 1,
        }
    }
    let missing = expected.len() - found;
    if damaged > 0 || missing > 0 {
        return Err(VaultError::Integrity(format!(
            "{damaged} damaged and {missing} missing records; restore a backup"
        )));
    }
    let mut addresses = HashSet::with_capacity(index.len());
    for entry in &index {
        let key = (
            entry.meta.project.as_str(),
            entry.meta.environment.as_str(),
            entry.meta.name.as_str(),
        );
        if !addresses.insert(key) {
            return Err(VaultError::Integrity(
                "two records have the same project, environment, and name".into(),
            ));
        }
    }
    Ok(index)
}

impl Unlocked {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn find(&self, address: &Address<'_>) -> Option<usize> {
        self.index
            .iter()
            .position(|entry| entry.meta.matches(address))
    }

    pub fn contains(&self, address: &Address<'_>) -> Result<bool> {
        address.check()?;
        Ok(self.find(address).is_some())
    }

    /// Secret addresses, optionally filtered, sorted by project, environment, and name.
    /// Values are never decrypted here.
    pub fn list(&self, project: Option<&str>, environment: Option<&str>) -> Vec<Address<'_>> {
        let mut out: Vec<Address<'_>> = self
            .index
            .iter()
            .filter(|e| project.is_none_or(|p| *e.meta.project == p))
            .filter(|e| environment.is_none_or(|env| *e.meta.environment == env))
            .map(|e| Address {
                project: &e.meta.project,
                environment: &e.meta.environment,
                name: &e.meta.name,
            })
            .collect();
        out.sort_by(|a, b| {
            (a.project, a.environment, a.name).cmp(&(b.project, b.environment, b.name))
        });
        out
    }

    /// Decrypts one value. The result is wiped when dropped.
    pub fn get(&self, address: &Address<'_>) -> Result<SecretBytes> {
        address.check()?;
        let entry = &self.index[self.find(address).ok_or(VaultError::NotFound)?];
        let damaged = || VaultError::Integrity("a record failed authentication".into());
        let aad = crypto::record_aad(&self.header.vault_id, &entry.id, FIELD_VALUE);
        let plain = crypto::open(&self.vk, &aad, &entry.value_blob).map_err(|_| damaged())?;
        let value = crypto::unpad(plain.expose()).map_err(|_| damaged())?;
        Ok(SecretBytes::new(Zeroizing::new(value.to_vec())))
    }

    /// Creates or replaces a secret. Both blobs get fresh nonces, so the stored ciphertext
    /// changes even when the value is the same. Returns `true` when the secret is new.
    pub fn set(&mut self, address: &Address<'_>, value: &[u8]) -> Result<bool> {
        address.check()?;
        check_value(value)?;
        let now = unix_now();
        let existing = self.find(address);
        let (id, created) = match existing {
            Some(position) => (self.index[position].id, self.index[position].meta.created),
            None => (self.new_record_id()?, now),
        };
        let meta = Meta {
            project: Zeroizing::new(address.project.to_owned()),
            environment: Zeroizing::new(address.environment.to_owned()),
            name: Zeroizing::new(address.name.to_owned()),
            notes: Zeroizing::new(String::new()),
            created,
            updated: now,
        };
        let vault_id = &self.header.vault_id;
        let meta_blob = crypto::seal(
            &self.vk,
            &crypto::record_aad(vault_id, &id, FIELD_META),
            &crypto::pad(&meta.encode()),
        )?;
        let value_blob = crypto::seal(
            &self.vk,
            &crypto::record_aad(vault_id, &id, FIELD_VALUE),
            &crypto::pad(value),
        )?;
        let entry = Entry {
            id,
            meta,
            meta_blob,
            value_blob,
        };
        let change = Change::Put(RawRecord {
            id,
            meta: entry.meta_blob.clone(),
            value: Some(entry.value_blob.clone()),
        });
        let mut digests = self.digests();
        digests.insert(id, entry.digest());
        self.commit(&change, &digests)?;
        match existing {
            Some(position) => self.index[position] = entry,
            None => self.index.push(entry),
        }
        Ok(existing.is_none())
    }

    pub fn delete(&mut self, address: &Address<'_>) -> Result<()> {
        address.check()?;
        let position = self.find(address).ok_or(VaultError::NotFound)?;
        let id = self.index[position].id;
        let mut digests = self.digests();
        digests.remove(&id);
        self.commit(&Change::Delete(id), &digests)?;
        self.index.swap_remove(position);
        Ok(())
    }

    fn digests(&self) -> BTreeMap<RecordId, Digest> {
        self.index.iter().map(|e| (e.id, e.digest())).collect()
    }

    fn new_record_id(&self) -> Result<RecordId> {
        loop {
            let id: RecordId = crypto::random_array()?;
            if self.index.iter().all(|e| e.id != id) {
                return Ok(id);
            }
        }
    }

    /// Writes one change with the next manifest generation in one transaction (SR-012).
    fn commit(&mut self, change: &Change, digests: &BTreeMap<RecordId, Digest>) -> Result<()> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| VaultError::Integrity("the generation counter is exhausted".into()))?;
        let manifest = seal_manifest(&self.vk, &self.header, generation, digests)?;
        self.store.write(&self.manifest_blob, change, &manifest)?;
        self.manifest_blob = manifest;
        self.generation = generation;
        Ok(())
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

    // ----- Secret records (002C) -----

    const VALUE: &[u8] = b"MK-CANARY-value-5d2b8e41a9c3";
    const PROJECT: &str = "MK-CANARY-project";
    const ENVIRONMENT: &str = "MK-CANARY-env";
    const NAME: &str = "MK-CANARY-NAME";

    fn at<'a>(project: &'a str, environment: &'a str, name: &'a str) -> Address<'a> {
        Address {
            project,
            environment,
            name,
        }
    }

    fn open_vault(path: &Path) -> Unlocked {
        unlock(path, Credential::Passphrase(&passphrase(PASS))).unwrap()
    }

    fn rows(path: &Path) -> Vec<RawRecord> {
        Store::open(path, false).unwrap().records().unwrap()
    }

    fn listed(vault: &Unlocked, project: Option<&str>, environment: Option<&str>) -> Vec<String> {
        vault
            .list(project, environment)
            .iter()
            .map(|a| format!("{}/{}/{}", a.project, a.environment, a.name))
            .collect()
    }

    /// US2: set, list, get, update, and delete, persisted across unlocks.
    #[test]
    fn secrets_round_trip_through_set_list_get_and_delete() {
        let (_dir, path) = new_vault(None);
        let mut vault = open_vault(&path);
        assert!(
            vault
                .set(&at("shop", "prod", "STRIPE_KEY"), b"sk_live_1")
                .unwrap()
        );
        assert!(
            vault
                .set(&at("shop", "dev", "STRIPE_KEY"), b"sk_test_1")
                .unwrap()
        );
        assert!(
            vault
                .set(&at("blog", "prod", "DB_URL"), b"postgres://x")
                .unwrap()
        );
        assert_eq!(vault.generation(), 3);
        drop(vault);

        let mut vault = open_vault(&path);
        assert_eq!(
            listed(&vault, None, None),
            [
                "blog/prod/DB_URL",
                "shop/dev/STRIPE_KEY",
                "shop/prod/STRIPE_KEY"
            ]
        );
        assert_eq!(
            listed(&vault, Some("shop"), None),
            ["shop/dev/STRIPE_KEY", "shop/prod/STRIPE_KEY"]
        );
        assert_eq!(
            listed(&vault, Some("shop"), Some("prod")),
            ["shop/prod/STRIPE_KEY"]
        );
        assert!(listed(&vault, Some("nope"), None).is_empty());
        let prod = at("shop", "prod", "STRIPE_KEY");
        assert_eq!(vault.get(&prod).unwrap().expose(), b"sk_live_1");
        // Names are case-sensitive.
        assert!(matches!(
            vault.get(&at("shop", "prod", "stripe_key")).unwrap_err(),
            VaultError::NotFound
        ));
        assert!(!vault.set(&prod, b"sk_live_2").unwrap());
        vault.delete(&at("blog", "prod", "DB_URL")).unwrap();
        assert_eq!(vault.generation(), 5);
        drop(vault);

        let mut vault = open_vault(&path);
        assert_eq!(vault.get(&prod).unwrap().expose(), b"sk_live_2");
        assert_eq!(listed(&vault, None, None).len(), 2);
        assert!(matches!(
            vault.delete(&at("blog", "prod", "DB_URL")).unwrap_err(),
            VaultError::NotFound
        ));
        assert_eq!(rows(&path).len(), 2);
    }

    /// US2 scenario 5: an update re-encrypts with fresh nonces, keeping the record id.
    #[test]
    fn updating_with_the_same_value_changes_the_ciphertext() {
        let (_dir, path) = new_vault(None);
        let mut vault = open_vault(&path);
        let address = at(PROJECT, ENVIRONMENT, NAME);
        vault.set(&address, VALUE).unwrap();
        let before = rows(&path);
        vault.set(&address, VALUE).unwrap();
        let after = rows(&path);
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].id, after[0].id);
        assert_ne!(before[0].meta, after[0].meta);
        assert_ne!(before[0].value, after[0].value);
        assert_eq!(vault.get(&address).unwrap().expose(), VALUE);
    }

    /// ST-02: no value, name, project, environment, or passphrase in any file, in any of
    /// several encodings, after set, update, and delete.
    #[test]
    fn vault_files_contain_no_secret_material() {
        let (dir, path) = new_vault(None);
        let mut vault = open_vault(&path);
        let address = at(PROJECT, ENVIRONMENT, NAME);
        let other = at(PROJECT, ENVIRONMENT, "MK-CANARY-OTHER");
        vault.set(&address, VALUE).unwrap();
        vault.set(&other, b"MK-CANARY-second-value").unwrap();
        vault.set(&address, b"MK-CANARY-updated-value").unwrap();
        vault.delete(&other).unwrap();
        drop(vault);
        let needles: [&[u8]; 8] = [
            VALUE,
            b"MK-CANARY-second-value",
            b"MK-CANARY-updated-value",
            PROJECT.as_bytes(),
            ENVIRONMENT.as_bytes(),
            NAME.as_bytes(),
            b"MK-CANARY-OTHER",
            PASS.as_bytes(),
        ];
        let data_dir = path.parent().unwrap();
        let files: Vec<_> = fs::read_dir(data_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(
            files,
            vec![path.clone()],
            "no journal or temporary files remain"
        );
        let bytes = fs::read(&path).unwrap();
        for needle in needles {
            let utf16: Vec<u8> = String::from_utf8_lossy(needle)
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect();
            let hex: String = needle.iter().map(|b| format!("{b:02x}")).collect();
            let b64 = data_encoding::BASE64.encode(needle);
            let forms: [&[u8]; 5] = [needle, &utf16, hex.as_bytes(), b64.as_bytes(), b"MK-CANARY"];
            for form in forms {
                assert!(
                    !contains(&bytes, form),
                    "secret material found in the vault"
                );
            }
        }
        drop(dir);
    }

    /// ST-03 (records): flipping any bit of any record blob is detected at unlock.
    #[test]
    fn tampering_with_any_record_byte_is_detected() {
        let (_dir, path) = new_vault(None);
        let mut vault = open_vault(&path);
        vault.set(&at("shop", "prod", "KEY"), b"value one").unwrap();
        vault
            .set(&at("shop", "prod", "OTHER"), b"value two")
            .unwrap();
        drop(vault);
        let pristine = fs::read(&path).unwrap();
        let all = rows(&path);
        let record = &all[0];
        let filter = format!(
            "record_id = x'{}'",
            record
                .id
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        for (column, len) in [
            ("meta_blob", record.meta.len()),
            ("value_blob", record.value.as_ref().unwrap().len()),
        ] {
            for index in 0..len {
                fs::write(&path, &pristine).unwrap();
                flip_byte(&path, "records", column, &filter, index);
                let error = unlock(&path, Credential::Passphrase(&passphrase(PASS)))
                    .expect_err(&format!("{column}[{index}] was accepted"));
                assert_eq!(error.exit_code(), 4, "{column}[{index}]: {error}");
            }
        }
        // The record id itself is bound too.
        fs::write(&path, &pristine).unwrap();
        sql(
            &path,
            &format!("UPDATE records SET record_id = zeroblob(16) WHERE {filter}"),
        );
        assert_eq!(
            unlock(&path, Credential::Passphrase(&passphrase(PASS)))
                .unwrap_err()
                .exit_code(),
            4
        );
        fs::write(&path, &pristine).unwrap();
        open_vault(&path);
    }

    fn set_blob(path: &Path, column: &str, id: &[u8; ID_LEN], blob: &[u8]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute(
            &format!("UPDATE records SET {column} = ?1 WHERE record_id = ?2"),
            (blob, id.as_slice()),
        )
        .unwrap();
    }

    fn assert_integrity_failure(path: &Path, what: &str) {
        let error = unlock(path, Credential::Passphrase(&passphrase(PASS)))
            .expect_err(&format!("{what} was accepted"));
        assert_eq!(error.exit_code(), 4, "{what}: {error}");
        let text = format!("{error} {error:?}");
        assert!(!text.contains("MK-CANARY"), "{what}: {text}");
    }

    /// ST-04 and ST-05: swapped blobs, blobs from another vault, a rolled-back record,
    /// a deleted row, and a copied row are all detected (exit 4).
    #[test]
    fn swapped_rolled_back_deleted_or_copied_records_are_detected() {
        let (_dir, path) = new_vault(None);
        let mut vault = open_vault(&path);
        vault
            .set(&at(PROJECT, ENVIRONMENT, "A"), b"MK-CANARY-a")
            .unwrap();
        vault
            .set(&at(PROJECT, ENVIRONMENT, "B"), b"MK-CANARY-b")
            .unwrap();
        drop(vault);
        let old_rows = rows(&path);
        let mut vault = open_vault(&path);
        vault
            .set(&at(PROJECT, ENVIRONMENT, "A"), b"MK-CANARY-a2")
            .unwrap();
        drop(vault);
        let pristine = fs::read(&path).unwrap();
        let current = rows(&path);
        let (a, b) = (&current[0], &current[1]);

        // ST-04: swap metadata or values between two records.
        for column in ["meta_blob", "value_blob"] {
            fs::write(&path, &pristine).unwrap();
            let pick = |r: &RawRecord| {
                if column == "meta_blob" {
                    r.meta.clone()
                } else {
                    r.value.clone().unwrap()
                }
            };
            set_blob(&path, column, &a.id, &pick(b));
            set_blob(&path, column, &b.id, &pick(a));
            assert_integrity_failure(&path, &format!("swapped {column}"));
        }

        // ST-04: a record copied from another vault (same record id).
        let (_other_dir, other) = new_vault(None);
        let mut other_vault = open_vault(&other);
        other_vault
            .set(&at(PROJECT, ENVIRONMENT, "A"), b"MK-CANARY-x")
            .unwrap();
        drop(other_vault);
        let foreign_rows = rows(&other);
        let foreign = &foreign_rows[0];
        fs::write(&path, &pristine).unwrap();
        set_blob(&path, "meta_blob", &a.id, &foreign.meta);
        set_blob(&path, "value_blob", &a.id, foreign.value.as_ref().unwrap());
        assert_integrity_failure(&path, "a record from another vault");

        // ST-05: roll one record back to its earlier stored version.
        let older = old_rows
            .iter()
            .find(|r| r.meta != a.meta && r.meta != b.meta)
            .unwrap();
        fs::write(&path, &pristine).unwrap();
        set_blob(&path, "meta_blob", &older.id, &older.meta);
        set_blob(
            &path,
            "value_blob",
            &older.id,
            older.value.as_ref().unwrap(),
        );
        assert_integrity_failure(&path, "a rolled-back record");

        // ST-05: delete a row; insert a copy of a row under a new id; drop a value.
        for statement in [
            "DELETE FROM records WHERE record_id = (SELECT record_id FROM records LIMIT 1)"
                .to_owned(),
            "INSERT INTO records SELECT randomblob(16), meta_blob, value_blob FROM records LIMIT 1"
                .to_owned(),
            "UPDATE records SET value_blob = NULL".to_owned(),
        ] {
            fs::write(&path, &pristine).unwrap();
            sql(&path, &statement);
            assert_integrity_failure(&path, &statement);
        }
        fs::write(&path, &pristine).unwrap();
        assert_eq!(
            open_vault(&path)
                .get(&at(PROJECT, ENVIRONMENT, "A"))
                .unwrap()
                .expose(),
            b"MK-CANARY-a2"
        );
    }

    /// Edge case "concurrent CLI writers": a writer whose unlock is stale is refused and
    /// the other writer's change survives.
    #[test]
    fn a_stale_writer_is_refused() {
        let (_dir, path) = new_vault(None);
        let mut first = open_vault(&path);
        let mut second = open_vault(&path);
        first.set(&at("p", "e", "ONE"), b"1").unwrap();
        let error = second.set(&at("p", "e", "TWO"), b"2").unwrap_err();
        assert_eq!(error.exit_code(), 4);
        drop((first, second));
        let vault = open_vault(&path);
        assert_eq!(listed(&vault, None, None), ["p/e/ONE"]);
        assert_eq!(vault.generation(), 1);
    }

    /// FR-003 limits; errors never repeat names or values.
    #[test]
    fn names_and_values_are_validated() {
        let (_dir, path) = new_vault(None);
        let mut vault = open_vault(&path);
        let long = "N".repeat(MAX_NAME_CHARS + 1);
        let max = "é".repeat(MAX_NAME_CHARS);
        for address in [
            at("", "e", "n"),
            at("p", "", "n"),
            at("p", "e", ""),
            at("p", "e", &long),
            at("p", "e", "MK-CANARY\nname"),
            at("p\u{7f}", "e", "n"),
        ] {
            let error = vault.set(&address, b"x").unwrap_err();
            assert_eq!(error.exit_code(), 2);
            assert!(!format!("{error} {error:?}").contains("MK-CANARY"));
        }
        for value in [Vec::new(), vec![0u8; MAX_VALUE_LEN + 1]] {
            assert_eq!(
                vault
                    .set(&at("p", "e", "n"), &value)
                    .unwrap_err()
                    .exit_code(),
                2
            );
        }
        let big = vec![0xA5u8; MAX_VALUE_LEN];
        vault.set(&at(&max, &max, &max), &big).unwrap();
        assert_eq!(
            vault.get(&at(&max, &max, &max)).unwrap().expose(),
            big.as_slice()
        );
        assert_eq!(vault.generation(), 1, "rejected input never wrote");
        let error = vault.get(&at("p", "e", "MK-CANARY-missing")).unwrap_err();
        assert!(matches!(error, VaultError::NotFound));
        assert!(!format!("{error} {error:?}").contains("MK-CANARY"));
        drop(vault);
        open_vault(&path);
    }

    /// ST-01: frozen layouts for record metadata and manifest entries.
    #[test]
    fn frozen_record_and_manifest_layouts() {
        let meta = Meta {
            project: Zeroizing::new("p".into()),
            environment: Zeroizing::new("env".into()),
            name: Zeroizing::new("KEY".into()),
            notes: Zeroizing::new(String::new()),
            created: 1,
            updated: 0x0102,
        };
        let encoded = meta.encode();
        let expected: Vec<u8> = [
            &[1u8][..],
            &1u64.to_le_bytes(),
            &0x0102u64.to_le_bytes(),
            &[1, 0],
            b"p",
            &[3, 0],
            b"env",
            &[3, 0],
            b"KEY",
            &[0, 0, 0, 0],
        ]
        .concat();
        assert_eq!(*encoded, expected, "metadata layout changed");
        let decoded = Meta::decode(&encoded).unwrap();
        assert!(decoded.matches(&at("p", "env", "KEY")));
        assert_eq!((decoded.created, decoded.updated), (1, 0x0102));
        assert!(Meta::decode(&encoded[..encoded.len() - 1]).is_err());
        assert!(Meta::decode(&[encoded.as_slice(), &[0]].concat()).is_err());

        let mut entries = BTreeMap::new();
        entries.insert([2; ID_LEN], [0xBB; DIGEST_LEN]);
        entries.insert([1; ID_LEN], [0xAA; DIGEST_LEN]);
        let manifest = encode_manifest(7, &entries);
        let expected: Vec<u8> = [
            &[1u8][..],
            &7u64.to_le_bytes(),
            &2u32.to_le_bytes(),
            &[1; ID_LEN],
            &[0xAA; DIGEST_LEN],
            &[2; ID_LEN],
            &[0xBB; DIGEST_LEN],
        ]
        .concat();
        assert_eq!(manifest, expected, "manifest layout changed");
        assert_eq!(decode_manifest(&manifest).unwrap(), (7, entries));
        // Entries out of order or repeated are refused.
        let mut swapped = manifest.clone();
        swapped[MANIFEST_HEADER_LEN..].rotate_left(MANIFEST_ENTRY_LEN);
        assert!(decode_manifest(&swapped).is_err());
        let mut repeated = manifest;
        repeated.copy_within(
            MANIFEST_HEADER_LEN..MANIFEST_HEADER_LEN + MANIFEST_ENTRY_LEN,
            MANIFEST_HEADER_LEN + MANIFEST_ENTRY_LEN,
        );
        assert!(decode_manifest(&repeated).is_err());
    }

    /// ST-14 (records): debug output of unlocked state never shows names or values.
    #[test]
    fn unlocked_records_are_redacted_in_debug_output() {
        let (_dir, path) = new_vault(None);
        let mut vault = open_vault(&path);
        vault.set(&at(PROJECT, ENVIRONMENT, NAME), VALUE).unwrap();
        let value = vault.get(&at(PROJECT, ENVIRONMENT, NAME)).unwrap();
        let text = format!("{vault:?} {:?} {value:?}", vault.index[0].meta);
        assert!(text.contains("[REDACTED]"));
        assert!(!text.contains("MK-CANARY"), "{text}");
    }

    /// Everything a user can observe: the generation and every address with its value.
    fn observed(path: &Path) -> (u64, Vec<(String, Vec<u8>)>) {
        let vault = open_vault(path);
        let mut seen = Vec::new();
        for address in vault.list(None, None) {
            let value = vault.get(&address).unwrap().expose().to_vec();
            seen.push((
                format!(
                    "{}/{}/{}",
                    address.project, address.environment, address.name
                ),
                value,
            ));
        }
        (vault.generation(), seen)
    }

    type WriteOp = fn(&mut Unlocked) -> Result<()>;

    /// ST-09: a write that fails or crashes between any two of its statements leaves the
    /// vault at the previous generation (or the next one once committed), never mixed, and
    /// the full unlock verification passes.
    #[test]
    fn interrupted_writes_leave_a_whole_generation() {
        let ops: [(&str, WriteOp); 3] = [
            ("create", |v| {
                v.set(&at("p", "e", "NEW"), &[0x11; 40_000]).map(drop)
            }),
            ("update", |v| {
                v.set(&at("p", "e", "KEEP"), b"changed").map(drop)
            }),
            ("delete", |v| v.delete(&at("p", "e", "GONE"))),
        ];
        for (name, op) in ops {
            for point in store::fault::ALL {
                let place = format!("{name} interrupted at point {point}");
                let (dir, path) = new_vault(None);
                let mut vault = open_vault(&path);
                vault
                    .set(&at("p", "e", "KEEP"), &[0x5A; MAX_VALUE_LEN])
                    .unwrap();
                vault.set(&at("p", "e", "GONE"), b"to delete").unwrap();
                drop(vault);
                let before = observed(&path);

                // At the chosen point, copy the vault and its rollback journal as a crash
                // would leave them, then fail the write.
                let crash_dir = dir.path().join("crash");
                fs::create_dir(&crash_dir).unwrap();
                let (source, target) = (path.clone(), crash_dir.clone());
                let mut vault = open_vault(&path);
                vault.store.spill_early();
                store::fault::install(Some(Box::new(move |at_point| {
                    if at_point != point {
                        return false;
                    }
                    for suffix in ["", "-journal"] {
                        let from = PathBuf::from(format!("{}{suffix}", source.display()));
                        if from.exists() {
                            fs::copy(&from, target.join(format!("vault.sqlite3{suffix}"))).unwrap();
                        }
                    }
                    true
                })));
                let result = op(&mut vault);
                store::fault::install(None);
                drop(vault);
                assert!(
                    result.is_err(),
                    "{place}: the injected failure was not reported"
                );

                // Two identical crashed copies: one is first opened by `status` (read-only),
                // the other directly by unlock, so each recovery path is exercised alone.
                let unlock_dir = dir.path().join("crash-unlock");
                fs::create_dir(&unlock_dir).unwrap();
                for entry in fs::read_dir(&crash_dir).unwrap() {
                    let entry = entry.unwrap();
                    fs::copy(entry.path(), unlock_dir.join(entry.file_name())).unwrap();
                }
                let crashed = crash_dir.join("vault.sqlite3");
                assert!(
                    matches!(status(&crashed), Ok(VaultStatus::Initialized { .. })),
                    "{place}: status of the crashed copy: {:?}",
                    status(&crashed)
                );
                let after = observed(&path);
                let recovered = observed(&unlock_dir.join("vault.sqlite3"));
                assert_eq!(
                    observed(&crashed),
                    recovered,
                    "{place}: status and unlock recovered differently"
                );
                if point == store::fault::COMMITTED {
                    assert_eq!(after.0, before.0 + 1, "{place}");
                    assert_eq!(recovered, after, "{place}");
                } else {
                    assert_eq!(
                        after, before,
                        "{place}: the failed write was not rolled back"
                    );
                    assert_eq!(
                        recovered, before,
                        "{place}: the crashed copy did not recover the previous generation"
                    );
                }
            }
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
