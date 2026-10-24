//! SQLite access. This module stores and loads ciphertext and plain structure only;
//! it never sees decrypted data (architecture: "SQLite configuration").

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use crate::crypto::{ID_LEN, KdfParams, SALT_LEN};

/// "MKV1" — identifies MaxonKey vault files (`PRAGMA application_id`).
pub const APPLICATION_ID: i32 = 0x4D4B_5631;
pub const SCHEMA_VERSION: i32 = 1;
pub const FORMAT_VERSION: u32 = 1;
pub const SLOT_PASSPHRASE: u8 = 1;
pub const SLOT_RECOVERY: u8 = 2;
pub const KDF_NONE: u8 = 0;

const SCHEMA: &str = "
CREATE TABLE vault_header (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    format_version INTEGER NOT NULL,
    vault_id BLOB NOT NULL CHECK (length(vault_id) = 16)
) STRICT;
CREATE TABLE key_slots (
    slot_id INTEGER PRIMARY KEY CHECK (slot_id IN (1, 2)),
    kdf_id INTEGER NOT NULL,
    kdf_m INTEGER NOT NULL,
    kdf_t INTEGER NOT NULL,
    kdf_p INTEGER NOT NULL,
    salt BLOB NOT NULL CHECK (length(salt) = 16),
    wrapped_vk BLOB NOT NULL
) STRICT;
CREATE TABLE records (
    record_id BLOB PRIMARY KEY CHECK (length(record_id) = 16),
    meta_blob BLOB NOT NULL,
    value_blob BLOB
) STRICT, WITHOUT ROWID;
CREATE TABLE manifest (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    blob BLOB NOT NULL
) STRICT;
";

#[derive(Debug)]
pub enum StoreError {
    /// The file is not a MaxonKey vault, uses an unknown version, or fails SQLite's check.
    Integrity(String),
    Sqlite(rusqlite::Error),
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub format_version: u32,
    pub vault_id: [u8; ID_LEN],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub slot_id: u8,
    pub kdf_id: u8,
    pub params: KdfParams,
    pub salt: [u8; SALT_LEN],
    pub wrapped_vk: Vec<u8>,
}

/// One `records` row: ciphertext only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord {
    pub id: [u8; ID_LEN],
    pub meta: Vec<u8>,
    pub value: Option<Vec<u8>>,
}

/// A single record change written together with a new manifest.
#[derive(Debug)]
pub enum Change {
    /// Insert a new record or replace the blobs of an existing one.
    Put(RawRecord),
    Delete([u8; ID_LEN]),
}

fn configure(conn: &Connection, writable: bool) -> Result<()> {
    conn.pragma_update(None, "trusted_schema", "OFF")?;
    conn.pragma_update(None, "secure_delete", "ON")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    if writable {
        conn.pragma_update(None, "journal_mode", "DELETE")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
    }
    Ok(())
}

/// Creates the schema in an empty database file (used only during initialization).
pub fn create(path: &Path, header: &Header, slots: &[Slot], manifest: &[u8]) -> Result<()> {
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&conn, true)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.execute_batch(SCHEMA)?;
    tx.execute(
        "INSERT INTO vault_header (singleton, format_version, vault_id) VALUES (1, ?1, ?2)",
        params![header.format_version, header.vault_id.as_slice()],
    )?;
    for slot in slots {
        insert_slot(&tx, slot)?;
    }
    tx.execute(
        "INSERT INTO manifest (singleton, blob) VALUES (1, ?1)",
        params![manifest],
    )?;
    tx.commit()?;
    conn.close().map_err(|(_, error)| error)?;
    Ok(())
}

fn insert_slot(conn: &Connection, slot: &Slot) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO key_slots (slot_id, kdf_id, kdf_m, kdf_t, kdf_p, salt, wrapped_vk)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            slot.slot_id,
            slot.kdf_id,
            slot.params.m_kib,
            slot.params.t,
            slot.params.p,
            slot.salt.as_slice(),
            slot.wrapped_vk
        ],
    )?;
    Ok(())
}

/// An open vault database whose identity, version, and structure have been checked.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path, writable: bool) -> Result<Self> {
        let mut flags = OpenFlags::SQLITE_OPEN_NO_MUTEX;
        flags |= if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let conn = Connection::open_with_flags(path, flags)?;
        configure(&conn, writable)?;
        let not_vault = || StoreError::Integrity("the file is not a MaxonKey vault".into());
        let application_id: i32 = conn
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|_| not_vault())?;
        if application_id != APPLICATION_ID {
            return Err(not_vault());
        }
        let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(StoreError::Integrity(format!(
                "unsupported vault schema version {version}; this MaxonKey supports {SCHEMA_VERSION}"
            )));
        }
        let check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            return Err(StoreError::Integrity(
                "SQLite structure check failed; restore a backup".into(),
            ));
        }
        Ok(Self { conn })
    }

    pub fn header(&self) -> Result<Header> {
        let (format_version, vault_id): (u32, Vec<u8>) = self
            .conn
            .query_row(
                "SELECT format_version, vault_id FROM vault_header WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::Integrity("vault header is missing".into()))?;
        if format_version != FORMAT_VERSION {
            return Err(StoreError::Integrity(format!(
                "unsupported vault format version {format_version}"
            )));
        }
        Ok(Header {
            format_version,
            vault_id: fixed(vault_id, "vault id")?,
        })
    }

    pub fn slot(&self, slot_id: u8) -> Result<Option<Slot>> {
        let row = self
            .conn
            .query_row(
                "SELECT kdf_id, kdf_m, kdf_t, kdf_p, salt, wrapped_vk FROM key_slots WHERE slot_id = ?1",
                params![slot_id],
                |row| {
                    Ok((
                        row.get::<_, u8>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, Vec<u8>>(4)?,
                        row.get::<_, Vec<u8>>(5)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(kdf_id, m_kib, t, p, salt, wrapped_vk)| {
            Ok(Slot {
                slot_id,
                kdf_id,
                params: KdfParams { m_kib, t, p },
                salt: fixed(salt, "salt")?,
                wrapped_vk,
            })
        })
        .transpose()
    }

    pub fn manifest(&self) -> Result<Vec<u8>> {
        self.conn
            .query_row("SELECT blob FROM manifest WHERE singleton = 1", [], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or_else(|| StoreError::Integrity("vault manifest is missing".into()))
    }

    /// Every stored record, ordered by record id (ciphertext only).
    pub fn records(&self) -> Result<Vec<RawRecord>> {
        let mut statement = self
            .conn
            .prepare("SELECT record_id, meta_blob, value_blob FROM records ORDER BY record_id")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (id, meta, value) = row?;
            records.push(RawRecord {
                id: fixed(id, "record id")?,
                meta,
                value,
            });
        }
        Ok(records)
    }

    /// Applies one record change and the new manifest in one `BEGIN IMMEDIATE` transaction
    /// (SR-012). Fails without writing if the manifest is no longer `expected_manifest`,
    /// that is, if another process changed the vault since it was unlocked.
    pub fn write(
        &mut self,
        expected_manifest: &[u8],
        change: &Change,
        new_manifest: &[u8],
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Vec<u8> =
            tx.query_row("SELECT blob FROM manifest WHERE singleton = 1", [], |row| {
                row.get(0)
            })?;
        if current != expected_manifest {
            return Err(StoreError::Integrity(
                "the vault was changed by another process; run the command again".into(),
            ));
        }
        let changed = match change {
            Change::Put(record) => tx.execute(
                "INSERT INTO records (record_id, meta_blob, value_blob) VALUES (?1, ?2, ?3)
                 ON CONFLICT (record_id) DO UPDATE SET meta_blob = ?2, value_blob = ?3",
                params![record.id.as_slice(), record.meta, record.value],
            )?,
            Change::Delete(id) => tx.execute(
                "DELETE FROM records WHERE record_id = ?1",
                params![id.as_slice()],
            )?,
        };
        if changed != 1 {
            return Err(StoreError::Integrity("record to change is missing".into()));
        }
        tx.execute(
            "UPDATE manifest SET blob = ?1 WHERE singleton = 1",
            params![new_manifest],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Replaces one key slot atomically. Fails if the header changed since unlock.
    pub fn replace_slot(&mut self, expected: &Header, slot: &Slot) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Vec<u8> = tx.query_row(
            "SELECT vault_id FROM vault_header WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        if current != expected.vault_id {
            return Err(StoreError::Integrity(
                "vault changed during the operation".into(),
            ));
        }
        insert_slot(&tx, slot)?;
        tx.commit()?;
        Ok(())
    }
}

fn fixed<const N: usize>(bytes: Vec<u8>, what: &str) -> Result<[u8; N]> {
    bytes
        .try_into()
        .map_err(|_| StoreError::Integrity(format!("vault {what} has the wrong length")))
}
