//! Types that hold sensitive bytes. They wipe their memory on drop and never print contents.

use std::fmt;

use unicode_normalization::UnicodeNormalization;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const KEY_LEN: usize = 32;
pub const MIN_PASSPHRASE_CHARS: usize = 12;

/// A 256-bit key kept on the heap so moves do not leave copies on the stack.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Key(Box<[u8; KEY_LEN]>);

impl Key {
    #[cfg(test)]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        let mut bytes = bytes;
        let key = Self(Box::new(bytes));
        bytes.zeroize();
        key
    }

    pub fn zeroed() -> Self {
        Self(Box::new([0; KEY_LEN]))
    }

    pub fn expose(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    pub fn expose_mut(&mut self) -> &mut [u8; KEY_LEN] {
        &mut self.0
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key([REDACTED])")
    }
}

/// Decrypted bytes of any length.
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    pub fn new(bytes: Zeroizing<Vec<u8>>) -> Self {
        Self(bytes)
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretBytes([REDACTED])")
    }
}

/// A master passphrase, normalized to Unicode NFC (SR-006).
pub struct Passphrase(Zeroizing<String>);

impl Passphrase {
    /// Normalizes the input and wipes the original buffer.
    pub fn from_input(input: Zeroizing<String>) -> Self {
        // NFC output is at most three times the input length; reserving avoids
        // reallocations that would leave unwiped copies behind.
        let mut normalized = Zeroizing::new(String::with_capacity(input.len() * 3));
        normalized.extend(input.nfc());
        Self(normalized)
    }

    pub fn char_count(&self) -> usize {
        self.0.chars().count()
    }

    pub fn expose(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn same_as(&self, other: &Self) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANARY: &str = "MK-CANARY-3f9c2a71d0b84e6f";

    #[test]
    fn debug_output_is_redacted() {
        let key = Key::from_bytes([0x41; KEY_LEN]);
        let bytes = SecretBytes::new(Zeroizing::new(CANARY.as_bytes().to_vec()));
        let pass = Passphrase::from_input(Zeroizing::new(CANARY.to_owned()));
        for text in [
            format!("{key:?}"),
            format!("{bytes:?}"),
            format!("{pass:?}"),
            format!("{:#?}", (&key, &bytes, &pass)),
        ] {
            assert!(text.contains("[REDACTED]"), "{text}");
            assert!(!text.contains(CANARY));
            assert!(!text.contains("41") && !text.contains("65"), "{text}");
        }
    }

    #[test]
    fn key_zeroize_clears_memory() {
        let mut key = Key::from_bytes([0xA5; KEY_LEN]);
        key.zeroize();
        assert_eq!(key.expose(), &[0; KEY_LEN]);
    }

    #[test]
    fn passphrase_is_nfc_normalized() {
        // "é" precomposed (NFC) and as "e" + combining acute (NFD).
        let nfc = Passphrase::from_input(Zeroizing::new("caf\u{e9} passphrase".to_owned()));
        let nfd = Passphrase::from_input(Zeroizing::new("cafe\u{301} passphrase".to_owned()));
        assert!(nfc.same_as(&nfd));
        assert_eq!(nfc.char_count(), 15);
    }

    #[test]
    fn passphrase_keeps_whitespace_and_case() {
        let a = Passphrase::from_input(Zeroizing::new(" Word word ".to_owned()));
        let b = Passphrase::from_input(Zeroizing::new("Word word".to_owned()));
        assert!(!a.same_as(&b));
    }
}
