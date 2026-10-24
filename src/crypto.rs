//! The only module that calls cryptographic crates (ADR 0001).
//!
//! - Argon2id v1.3 derives a key-encryption key (KEK) from the passphrase.
//! - XChaCha20-Poly1305 with a fresh random 24-byte nonce seals every blob.
//! - Blob layout: format byte ‖ nonce ‖ ciphertext ‖ tag.
//! - SHA-256 digests of stored record blobs go into the manifest.

use argon2::{Algorithm, Argon2, Block, Params, Version};
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::secret::{KEY_LEN, Key, Passphrase, SecretBytes};

pub const SALT_LEN: usize = 16;
pub const ID_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
pub const BLOB_FORMAT: u8 = 1;
pub const PAD_BLOCK: usize = 64;
pub const KDF_ARGON2ID: u8 = 1;
pub const DIGEST_LEN: usize = 32;
/// Field tags inside record AAD: which blob of a record a ciphertext belongs to.
pub const FIELD_META: u8 = 1;
pub const FIELD_VALUE: u8 = 2;

const AAD_PREFIX: &[u8; 12] = b"MaxonKey/v1/";

/// Argon2id cost parameters. `m_kib` is memory in KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// D3 defaults: 256 MiB, 3 passes, 1 lane.
    pub const DEFAULT: Self = Self {
        m_kib: 262_144,
        t: 3,
        p: 1,
    };
    const MIN_CREATE: Self = Self {
        m_kib: 65_536,
        t: 3,
        p: 1,
    };
    const MAX_OPEN: Self = Self {
        m_kib: 2_097_152,
        t: 10,
        p: 16,
    };

    /// Parameters accepted when creating or re-wrapping a passphrase slot.
    pub fn valid_for_create(self) -> bool {
        self.m_kib >= Self::MIN_CREATE.m_kib && self.t >= Self::MIN_CREATE.t && self.within_open()
    }

    /// Parameters accepted when opening a vault. Checked before any memory is allocated.
    pub fn within_open(self) -> bool {
        self.p >= 1
            && self.t >= 1
            && self.m_kib >= 8 * self.p
            && self.m_kib <= Self::MAX_OPEN.m_kib
            && self.t <= Self::MAX_OPEN.t
            && self.p <= Self::MAX_OPEN.p
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// Authentication failed: wrong key or modified data. Deliberately carries no detail.
    Auth,
    /// The blob is not in a format this version understands.
    Format,
    /// KDF parameters out of range or the KDF failed.
    Kdf,
    /// The operating system random generator failed.
    Random,
}

/// Fills `buf` from the operating system CSPRNG.
pub fn fill_random(buf: &mut [u8]) -> Result<(), CryptoError> {
    getrandom::fill(buf).map_err(|_| CryptoError::Random)
}

pub fn random_array<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut out = [0; N];
    fill_random(&mut out)?;
    Ok(out)
}

pub fn random_key() -> Result<Key, CryptoError> {
    let mut key = Key::zeroed();
    fill_random(key.expose_mut())?;
    Ok(key)
}

/// Derives a 32-byte KEK with Argon2id. The working memory is wiped afterwards.
pub fn derive_kek(
    passphrase: &Passphrase,
    salt: &[u8; SALT_LEN],
    params: KdfParams,
) -> Result<Key, CryptoError> {
    if !params.within_open() {
        return Err(CryptoError::Kdf);
    }
    let argon_params = Params::new(params.m_kib, params.t, params.p, Some(KEY_LEN))
        .map_err(|_| CryptoError::Kdf)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut memory = vec![Block::default(); argon.params().block_count()];
    let mut kek = Key::zeroed();
    let result = argon.hash_password_into_with_memory(
        passphrase.expose(),
        salt,
        kek.expose_mut(),
        &mut memory,
    );
    memory.zeroize();
    result.map_err(|_| CryptoError::Kdf)?;
    Ok(kek)
}

/// Encrypts `plaintext` and returns a self-describing blob.
pub fn seal(key: &Key, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let nonce: [u8; NONCE_LEN] = random_array()?;
    seal_with_nonce(key, aad, plaintext, &nonce)
}

fn seal_with_nonce(
    key: &Key,
    aad: &[u8],
    plaintext: &[u8],
    nonce: &[u8; NONCE_LEN],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| CryptoError::Auth)?;
    // Reserve room for the tag so encryption never reallocates and leaves plaintext behind.
    let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len() + TAG_LEN));
    buffer.extend_from_slice(plaintext);
    cipher
        .encrypt_in_place(&XNonce::from(*nonce), aad, &mut *buffer)
        .map_err(|_| CryptoError::Auth)?;
    let mut blob = Vec::with_capacity(1 + NONCE_LEN + buffer.len());
    blob.push(BLOB_FORMAT);
    blob.extend_from_slice(nonce);
    blob.extend_from_slice(&buffer);
    Ok(blob)
}

/// Authenticates and decrypts a blob produced by [`seal`].
pub fn open(key: &Key, aad: &[u8], blob: &[u8]) -> Result<SecretBytes, CryptoError> {
    if blob.len() < 1 + NONCE_LEN + TAG_LEN {
        return Err(CryptoError::Format);
    }
    if blob[0] != BLOB_FORMAT {
        return Err(CryptoError::Format);
    }
    let nonce = XNonce::try_from(&blob[1..1 + NONCE_LEN]).map_err(|_| CryptoError::Format)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| CryptoError::Auth)?;
    let mut buffer = Zeroizing::new(blob[1 + NONCE_LEN..].to_vec());
    cipher
        .decrypt_in_place(&nonce, aad, &mut *buffer)
        .map_err(|_| CryptoError::Auth)?;
    Ok(SecretBytes::new(buffer))
}

/// Length-prefixes and zero-pads plaintext to a multiple of 64 bytes (SR-011).
pub fn pad(data: &[u8]) -> Zeroizing<Vec<u8>> {
    let length = u32::try_from(data.len()).expect("plaintext larger than 4 GiB");
    let total = (4 + data.len()).div_ceil(PAD_BLOCK) * PAD_BLOCK;
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(data);
    out.resize(total, 0);
    out
}

/// Reverses [`pad`], rejecting any malformed padding.
pub fn unpad(padded: &[u8]) -> Result<&[u8], CryptoError> {
    if padded.len() < PAD_BLOCK || !padded.len().is_multiple_of(PAD_BLOCK) {
        return Err(CryptoError::Format);
    }
    let length = u32::from_le_bytes(padded[..4].try_into().expect("four bytes")) as usize;
    let end = 4usize.checked_add(length).ok_or(CryptoError::Format)?;
    if end > padded.len()
        || padded.len() - end >= PAD_BLOCK
        || padded[end..].iter().any(|b| *b != 0)
    {
        return Err(CryptoError::Format);
    }
    Ok(&padded[4..end])
}

/// What an encrypted value is used for. Part of every AAD so blobs cannot change role.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Purpose {
    KeySlot = 1,
    Manifest = 2,
    Record = 3,
}

/// SHA-256 over the concatenation of `parts`.
pub fn sha256(parts: &[&[u8]]) -> [u8; DIGEST_LEN] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// Fields bound into a key slot's associated data (SR-005).
pub struct SlotBinding<'a> {
    pub format_version: u32,
    pub vault_id: &'a [u8; ID_LEN],
    pub slot_id: u8,
    pub kdf_id: u8,
    pub params: KdfParams,
    pub salt: &'a [u8; SALT_LEN],
}

/// Fixed-length AAD for a key slot.
pub fn slot_aad(binding: &SlotBinding<'_>) -> Vec<u8> {
    let mut aad = Vec::with_capacity(12 + 1 + 4 + ID_LEN + 2 + 12 + SALT_LEN);
    aad.extend_from_slice(AAD_PREFIX);
    aad.push(Purpose::KeySlot as u8);
    aad.extend_from_slice(&binding.format_version.to_le_bytes());
    aad.extend_from_slice(binding.vault_id);
    aad.push(binding.slot_id);
    aad.push(binding.kdf_id);
    aad.extend_from_slice(&binding.params.m_kib.to_le_bytes());
    aad.extend_from_slice(&binding.params.t.to_le_bytes());
    aad.extend_from_slice(&binding.params.p.to_le_bytes());
    aad.extend_from_slice(binding.salt);
    aad
}

/// Fixed-length (46-byte) AAD for vault content: prefix ‖ purpose ‖ vault id ‖ record id ‖ field.
fn content_aad(
    purpose: Purpose,
    vault_id: &[u8; ID_LEN],
    record_id: &[u8; ID_LEN],
    field: u8,
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(12 + 1 + 2 * ID_LEN + 1);
    aad.extend_from_slice(AAD_PREFIX);
    aad.push(purpose as u8);
    aad.extend_from_slice(vault_id);
    aad.extend_from_slice(record_id);
    aad.push(field);
    aad
}

/// AAD for the manifest: the record id is all zeros and the field is 0.
pub fn manifest_aad(vault_id: &[u8; ID_LEN]) -> Vec<u8> {
    content_aad(Purpose::Manifest, vault_id, &[0; ID_LEN], 0)
}

/// AAD for one blob of a record (`FIELD_META` or `FIELD_VALUE`), so a blob cannot move to
/// another record, field, or vault.
pub fn record_aad(vault_id: &[u8; ID_LEN], record_id: &[u8; ID_LEN], field: u8) -> Vec<u8> {
    content_aad(Purpose::Record, vault_id, record_id, field)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Low-cost parameters for unit tests only; the CLI cannot select them.
    pub const TEST_PARAMS: KdfParams = KdfParams {
        m_kib: 64,
        t: 1,
        p: 1,
    };

    fn hex(text: &str) -> Vec<u8> {
        let clean: String = text.split_whitespace().collect();
        (0..clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).unwrap())
            .collect()
    }

    fn key_of(byte_start: u8) -> Key {
        let mut key = Key::zeroed();
        for (i, b) in key.expose_mut().iter_mut().enumerate() {
            *b = byte_start + i as u8;
        }
        key
    }

    fn passphrase(text: &str) -> Passphrase {
        Passphrase::from_input(Zeroizing::new(text.to_owned()))
    }

    /// ST-01: RFC 9106 §5.3 Argon2id test vector against the configured crate.
    #[test]
    fn argon2id_rfc9106_vector() {
        let params = argon2::ParamsBuilder::new()
            .m_cost(32)
            .t_cost(3)
            .p_cost(4)
            .data(argon2::AssociatedData::new(&[0x04; 12]).unwrap())
            .build()
            .unwrap();
        let ctx = Argon2::new_with_secret(&[0x03; 8], Algorithm::Argon2id, Version::V0x13, params)
            .unwrap();
        let mut out = [0u8; 32];
        ctx.hash_password_into(&[0x01; 32], &[0x02; 16], &mut out)
            .unwrap();
        assert_eq!(
            out.to_vec(),
            hex("0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659")
        );
    }

    /// ST-01: AEAD_XChaCha20_Poly1305 vector from draft-irtf-cfrg-xchacha-03.
    #[test]
    fn xchacha20poly1305_draft_vector() {
        let key = key_of(0x80);
        let nonce: [u8; NONCE_LEN] = hex("404142434445464748494a4b4c4d4e4f5051525354555657")
            .try_into()
            .unwrap();
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let expected = hex(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb
             731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452
             2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9
             21f9664c97637da9768812f615c68b13b52e
             c0875924c1c7987947deafd8780acf49",
        );
        let blob = seal_with_nonce(&key, &aad, plaintext, &nonce).unwrap();
        assert_eq!(blob[0], BLOB_FORMAT);
        assert_eq!(&blob[1..1 + NONCE_LEN], &nonce);
        assert_eq!(&blob[1 + NONCE_LEN..], expected.as_slice());
        assert_eq!(open(&key, &aad, &blob).unwrap().expose(), plaintext);
    }

    /// ST-01: frozen regression vectors for MaxonKey's own derivation and layouts.
    #[test]
    fn frozen_maxonkey_vectors() {
        let kek = derive_kek(
            &passphrase("correct horse battery"),
            &[7; SALT_LEN],
            TEST_PARAMS,
        )
        .unwrap();
        assert_eq!(
            kek.expose().to_vec(),
            hex(FROZEN_KEK),
            "KEK derivation changed; existing vaults would no longer open"
        );
        let binding = SlotBinding {
            format_version: 1,
            vault_id: &[0xAA; ID_LEN],
            slot_id: 1,
            kdf_id: KDF_ARGON2ID,
            params: KdfParams::DEFAULT,
            salt: &[0xBB; SALT_LEN],
        };
        let aad = slot_aad(&binding);
        assert_eq!(aad.len(), 63);
        assert_eq!(&aad[..13], b"MaxonKey/v1/\x01");
        let mut expected = b"MaxonKey/v1/\x02".to_vec();
        expected.extend_from_slice(&[0xAA; ID_LEN]);
        expected.extend_from_slice(&[0; ID_LEN + 1]);
        assert_eq!(
            manifest_aad(&[0xAA; ID_LEN]),
            expected,
            "manifest AAD changed"
        );
        let mut expected = b"MaxonKey/v1/\x03".to_vec();
        expected.extend_from_slice(&[0xAA; ID_LEN]);
        expected.extend_from_slice(&[0xCC; ID_LEN]);
        expected.push(FIELD_VALUE);
        assert_eq!(
            record_aad(&[0xAA; ID_LEN], &[0xCC; ID_LEN], FIELD_VALUE),
            expected,
            "record AAD changed"
        );
        assert_eq!(expected.len(), 46);
        let blob = seal_with_nonce(&key_of(0), &aad, b"layout", &[9; NONCE_LEN]).unwrap();
        assert_eq!(hex(FROZEN_BLOB), blob, "blob layout changed");
    }

    /// Cross-checked with the libargon2 C reference implementation (argon2-cffi).
    const FROZEN_KEK: &str = "562c92fc17ccb36c5048c3d45fe6745b4706422b3098c88099718e09412a1393";
    const FROZEN_BLOB: &str = "01 090909090909090909090909090909090909090909090909
         79414d10c93b 44269392c3b6670bdae598efd6ac305b";

    /// ST-01: FIPS 180-2 SHA-256 known answers; split input hashes like the joined input.
    #[test]
    fn sha256_known_answers() {
        assert_eq!(
            sha256(&[b"abc"]).to_vec(),
            hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(
            sha256(&[]).to_vec(),
            hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
        assert_eq!(sha256(&[b"a", b"bc"]), sha256(&[b"abc"]));
    }

    #[test]
    fn record_aad_differs_by_field_record_and_vault() {
        let base = record_aad(&[1; ID_LEN], &[2; ID_LEN], FIELD_META);
        for other in [
            record_aad(&[1; ID_LEN], &[2; ID_LEN], FIELD_VALUE),
            record_aad(&[1; ID_LEN], &[3; ID_LEN], FIELD_META),
            record_aad(&[9; ID_LEN], &[2; ID_LEN], FIELD_META),
            manifest_aad(&[1; ID_LEN]),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn wrong_key_aad_or_modified_blob_fails() {
        let key = key_of(1);
        let blob = seal(&key, b"aad", b"MK-CANARY-value").unwrap();
        assert_eq!(
            open(&key_of(2), b"aad", &blob).unwrap_err(),
            CryptoError::Auth
        );
        assert_eq!(open(&key, b"aaD", &blob).unwrap_err(), CryptoError::Auth);
        for index in 0..blob.len() {
            let mut tampered = blob.clone();
            tampered[index] ^= 0x01;
            assert!(open(&key, b"aad", &tampered).is_err(), "byte {index}");
        }
        assert_eq!(
            open(&key, b"aad", &blob[..40]).unwrap_err(),
            CryptoError::Format
        );
    }

    /// ST-08: nonces and ciphertexts never repeat.
    #[test]
    fn nonces_are_unique() {
        let key = key_of(3);
        let mut nonces = HashSet::new();
        let mut blobs = HashSet::new();
        for _ in 0..10_000 {
            let blob = seal(&key, b"", b"same plaintext").unwrap();
            assert!(nonces.insert(blob[1..1 + NONCE_LEN].to_vec()));
            assert!(blobs.insert(blob));
        }
    }

    #[test]
    fn padding_round_trips_and_rejects_malformed_input() {
        for len in [0, 1, 59, 60, 61, 64, 200] {
            let data = vec![0x5A; len];
            let padded = pad(&data);
            assert_eq!(padded.len() % PAD_BLOCK, 0);
            assert_eq!(unpad(&padded).unwrap(), data.as_slice());
        }
        let mut bad = pad(b"abc").to_vec();
        bad[10] = 1;
        assert!(unpad(&bad).is_err());
        let mut long = pad(b"abc").to_vec();
        long[0] = 200;
        assert!(unpad(&long).is_err());
        assert!(unpad(&[0; 63]).is_err());
        let mut extra = pad(b"abc").to_vec();
        extra.extend_from_slice(&[0; PAD_BLOCK]);
        assert!(unpad(&extra).is_err());
    }

    #[test]
    fn kdf_parameter_limits() {
        assert!(KdfParams::DEFAULT.valid_for_create());
        assert!(!TEST_PARAMS.valid_for_create());
        for params in [
            KdfParams {
                m_kib: 4 * 1024 * 1024,
                t: 3,
                p: 1,
            },
            KdfParams {
                m_kib: 65_536,
                t: 100,
                p: 1,
            },
            KdfParams {
                m_kib: 65_536,
                t: 3,
                p: 255,
            },
            KdfParams {
                m_kib: 65_536,
                t: 0,
                p: 1,
            },
            KdfParams {
                m_kib: 65_536,
                t: 3,
                p: 0,
            },
        ] {
            assert!(!params.within_open(), "{params:?}");
            assert_eq!(
                derive_kek(&passphrase("irrelevant passphrase"), &[0; SALT_LEN], params)
                    .unwrap_err(),
                CryptoError::Kdf
            );
        }
    }

    #[test]
    fn different_salts_or_passphrases_give_different_keys() {
        let a = derive_kek(&passphrase("passphrase one"), &[1; SALT_LEN], TEST_PARAMS).unwrap();
        let b = derive_kek(&passphrase("passphrase one"), &[2; SALT_LEN], TEST_PARAMS).unwrap();
        let c = derive_kek(&passphrase("passphrase two"), &[1; SALT_LEN], TEST_PARAMS).unwrap();
        assert_ne!(a.expose(), b.expose());
        assert_ne!(a.expose(), c.expose());
    }
}
