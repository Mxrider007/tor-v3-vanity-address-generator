//! Tor v3 onion addresses and hidden-service key files.
//!
//! An onion address is `base32(pubkey || checksum || version) + ".onion"` where
//! `checksum = SHA3-256(".onion checksum" || pubkey || version)[..2]` and
//! `version = 3` (see Tor's `rend-spec-v3.txt`). The 35 encoded bytes give
//! exactly 56 base32 characters.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use curve25519_dalek::{EdwardsPoint, Scalar};
use sha2::Sha512;
use sha3::{Digest, Sha3_256};
use thiserror::Error;
use zeroize::Zeroize;

/// Number of base32 characters in a v3 onion address (without `.onion`).
pub const ADDRESS_LEN: usize = 56;

/// The RFC 4648 base32 alphabet, lowercase, as used by onion addresses.
pub const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Number of leading address characters determined by the public key alone
/// (the rest depend on the checksum/version bytes).
pub const PUBKEY_CHARS: usize = 51;

/// 32-byte header of Tor's `hs_ed25519_secret_key` file.
pub const SECRET_KEY_HEADER: &[u8; 32] = b"== ed25519v1-secret: type0 ==\0\0\0";

/// 32-byte header of Tor's `hs_ed25519_public_key` file.
pub const PUBLIC_KEY_HEADER: &[u8; 32] = b"== ed25519v1-public: type0 ==\0\0\0";

const VERSION: u8 = 3;

/// The two-byte address checksum for a public key.
pub fn checksum(pubkey: &[u8; 32]) -> [u8; 2] {
    let hash = Sha3_256::new()
        .chain_update(b".onion checksum")
        .chain_update(pubkey)
        .chain_update([VERSION])
        .finalize();
    [hash[0], hash[1]]
}

/// The 56 lowercase base32 characters of the address for `pubkey`.
pub fn address_chars(pubkey: &[u8; 32]) -> [u8; ADDRESS_LEN] {
    let mut raw = [0u8; 35];
    raw[..32].copy_from_slice(pubkey);
    raw[32..34].copy_from_slice(&checksum(pubkey));
    raw[34] = VERSION;

    let mut out = [0u8; ADDRESS_LEN];
    for (group, chunk) in raw.as_chunks::<5>().0.iter().enumerate() {
        let v = chunk.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
        for i in 0..8 {
            out[group * 8 + i] = ALPHABET[((v >> (35 - 5 * i)) & 31) as usize];
        }
    }
    out
}

/// The first [`PUBKEY_CHARS`] address characters, which depend only on the
/// public key (no hashing needed).
pub fn pubkey_chars(pubkey: &[u8; 32]) -> [u8; PUBKEY_CHARS] {
    let mut out = [0u8; PUBKEY_CHARS];
    for (group, chunk) in pubkey[..30].as_chunks::<5>().0.iter().enumerate() {
        let v = chunk.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
        for i in 0..8 {
            out[group * 8 + i] = ALPHABET[((v >> (35 - 5 * i)) & 31) as usize];
        }
    }
    let tail = (u32::from(pubkey[30]) << 8) | u32::from(pubkey[31]);
    out[48] = ALPHABET[((tail >> 11) & 31) as usize];
    out[49] = ALPHABET[((tail >> 6) & 31) as usize];
    out[50] = ALPHABET[((tail >> 1) & 31) as usize];
    out
}

/// The full `xxxx….onion` address for a public key.
pub fn onion_address(pubkey: &[u8; 32]) -> String {
    let chars = address_chars(pubkey);
    let mut s = String::with_capacity(ADDRESS_LEN + 6);
    s.push_str(std::str::from_utf8(&chars).expect("base32 is ASCII"));
    s.push_str(".onion");
    s
}

/// Why an onion address failed to parse.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AddressError {
    #[error("an onion v3 address has 56 characters before \".onion\" (got {0})")]
    Length(usize),
    #[error("'{0}' is not a valid onion address character")]
    Character(char),
    #[error("unsupported onion address version {0} (only v3 is supported)")]
    Version(u8),
    #[error("checksum mismatch: this is not a valid onion address")]
    Checksum,
}

/// Parse and validate a v3 onion address (with or without the `.onion`
/// suffix, any case), returning its public key.
pub fn decode_onion_address(address: &str) -> Result<[u8; 32], AddressError> {
    let trimmed = address.trim();
    let lower = trimmed.to_ascii_lowercase();
    let body = lower.strip_suffix(".onion").unwrap_or(&lower);
    let len = body.chars().count();
    if len != ADDRESS_LEN {
        return Err(AddressError::Length(len));
    }

    let mut raw = [0u8; 35];
    for (group, chunk) in body.as_bytes().as_chunks::<8>().0.iter().enumerate() {
        let mut v = 0u64;
        for &c in chunk {
            let digit = ALPHABET
                .iter()
                .position(|&a| a == c)
                .ok_or(AddressError::Character(c as char))?;
            v = (v << 5) | digit as u64;
        }
        for i in 0..5 {
            raw[group * 5 + i] = (v >> (32 - 8 * i)) as u8;
        }
    }

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&raw[..32]);
    if raw[34] != VERSION {
        return Err(AddressError::Version(raw[34]));
    }
    if raw[32..34] != checksum(&pubkey) {
        return Err(AddressError::Checksum);
    }
    Ok(pubkey)
}

/// An Ed25519 secret key in Tor's 64-byte "expanded" form: the secret scalar
/// `a` (32 bytes, little-endian) followed by the nonce prefix `RH` (32 bytes).
///
/// The memory is wiped when the value is dropped.
pub struct SecretKey {
    expanded: [u8; 64],
}

impl SecretKey {
    /// Build a key from its scalar and nonce prefix. Tor uses the scalar
    /// as-is (it never re-clamps), so any scalar works; `RH` must be secret.
    pub fn from_parts(scalar: &[u8; 32], hash_prefix: &[u8; 32]) -> Self {
        let mut expanded = [0u8; 64];
        expanded[..32].copy_from_slice(scalar);
        expanded[32..].copy_from_slice(hash_prefix);
        Self { expanded }
    }

    /// Standard Ed25519 key expansion of a 32-byte seed (SHA-512, clamp).
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let mut hash: [u8; 64] = Sha512::digest(seed).into();
        clamp(&mut hash[..32]);
        let key = Self { expanded: hash };
        hash.zeroize();
        key
    }

    /// Wrap an existing 64-byte expanded key.
    pub fn from_expanded(expanded: [u8; 64]) -> Self {
        Self { expanded }
    }

    /// The secret scalar (first half of the expanded key).
    pub fn scalar(&self) -> &[u8; 32] {
        self.expanded[..32].try_into().expect("32 bytes")
    }

    /// The nonce prefix `RH` (second half of the expanded key).
    pub fn hash_prefix(&self) -> &[u8; 32] {
        self.expanded[32..].try_into().expect("32 bytes")
    }

    /// The 64 bytes Tor stores after the file header.
    pub fn expanded(&self) -> &[u8; 64] {
        &self.expanded
    }

    /// The Ed25519 public key `a·B`, computed with curve25519-dalek.
    pub fn public_key(&self) -> [u8; 32] {
        EdwardsPoint::mul_base(&Scalar::from_bytes_mod_order(*self.scalar()))
            .compress()
            .to_bytes()
    }

    /// The onion address of this key.
    pub fn onion_address(&self) -> String {
        onion_address(&self.public_key())
    }

    /// Sign a message exactly the way Tor does with an expanded key (the
    /// scalar is used without re-clamping) and verify the signature against
    /// `pubkey`. Returns `true` if the key is usable for `pubkey`.
    pub fn self_test(&self, pubkey: &[u8; 32]) -> bool {
        use ed25519_dalek::VerifyingKey;
        use ed25519_dalek::hazmat::{ExpandedSecretKey, raw_sign};

        let Ok(verifying_key) = VerifyingKey::from_bytes(pubkey) else {
            return false;
        };
        let esk = ExpandedSecretKey {
            scalar: Scalar::from_bytes_mod_order(*self.scalar()),
            hash_prefix: *self.hash_prefix(),
        };
        let message = b"torv3-vanity key self-test";
        let signature = raw_sign::<Sha512>(&esk, message, &verifying_key);
        verifying_key.verify_strict(message, &signature).is_ok()
    }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.expanded.zeroize();
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

/// Standard Ed25519 scalar clamping (in place).
pub(crate) fn clamp(scalar: &mut [u8]) {
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;
}

/// Whether a scalar is left unchanged by clamping. Keys produced by this crate
/// always are, so tools that (incorrectly) re-clamp still derive the same key.
pub fn is_clamped(scalar: &[u8; 32]) -> bool {
    scalar[0] & 7 == 0 && scalar[31] & 0xc0 == 0x40
}

/// Write a complete Tor v3 hidden-service directory for `secret` into
/// `<output_dir>/<address-without-.onion>/` and return that path.
///
/// The directory contains `hostname`, `hs_ed25519_public_key`,
/// `hs_ed25519_secret_key` and an empty `authorized_clients/`. On Unix the
/// directory is created `0700` and the files `0600` from the start, so the
/// secret key is never readable by other users, not even briefly. Existing
/// files are never overwritten, and everything is flushed to disk before
/// returning.
pub fn write_hidden_service_dir(output_dir: &Path, secret: &SecretKey) -> io::Result<PathBuf> {
    let pubkey = secret.public_key();
    let onion = onion_address(&pubkey);
    let hs_dir = output_dir.join(onion.trim_end_matches(".onion"));

    fs::create_dir_all(output_dir)?;
    create_private_dir(&hs_dir)?;
    write_new_file(&hs_dir.join("hostname"), format!("{onion}\n").as_bytes())?;

    let mut public = Vec::with_capacity(64);
    public.extend_from_slice(PUBLIC_KEY_HEADER);
    public.extend_from_slice(&pubkey);
    write_new_file(&hs_dir.join("hs_ed25519_public_key"), &public)?;

    let mut private = [0u8; 96];
    private[..32].copy_from_slice(SECRET_KEY_HEADER);
    private[32..].copy_from_slice(secret.expanded());
    let result = write_new_file(&hs_dir.join("hs_ed25519_secret_key"), &private);
    private.zeroize();
    result?;

    create_private_dir(&hs_dir.join("authorized_clients"))?;
    Ok(hs_dir)
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

fn write_new_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

/// Problems found when checking a hidden-service directory.
#[derive(Debug, Error)]
pub enum KeyFileError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0}: not a Tor ed25519 key file (wrong size or header)")]
    Format(PathBuf),
    #[error("the secret key does not match the public key file")]
    PublicKeyMismatch,
    #[error("the hostname file says {found} but the key is for {expected}")]
    HostnameMismatch { expected: String, found: String },
    #[error("the secret key failed the signing self-test")]
    SelfTest,
}

/// Check that a hidden-service directory is complete and self-consistent:
/// headers and sizes are right, the secret key derives the stored public key,
/// `hostname` matches, and the key can produce a valid signature. Returns the
/// onion address.
pub fn verify_hidden_service_dir(dir: &Path) -> Result<String, KeyFileError> {
    let read = |name: &str| {
        let path = dir.join(name);
        fs::read(&path).map_err(|source| KeyFileError::Io { path, source })
    };

    let secret_bytes = read("hs_ed25519_secret_key")?;
    if secret_bytes.len() != 96 || &secret_bytes[..32] != SECRET_KEY_HEADER {
        return Err(KeyFileError::Format(dir.join("hs_ed25519_secret_key")));
    }
    let mut expanded = [0u8; 64];
    expanded.copy_from_slice(&secret_bytes[32..]);
    let secret = SecretKey::from_expanded(expanded);
    expanded.zeroize();

    let public_bytes = read("hs_ed25519_public_key")?;
    if public_bytes.len() != 64 || &public_bytes[..32] != PUBLIC_KEY_HEADER {
        return Err(KeyFileError::Format(dir.join("hs_ed25519_public_key")));
    }
    let pubkey = secret.public_key();
    if public_bytes[32..] != pubkey {
        return Err(KeyFileError::PublicKeyMismatch);
    }

    let expected = onion_address(&pubkey);
    let hostname = read("hostname")?;
    let found = String::from_utf8_lossy(&hostname).trim().to_string();
    if found != expected {
        return Err(KeyFileError::HostnameMismatch { expected, found });
    }

    if !secret.self_test(&pubkey) {
        return Err(KeyFileError::SelfTest);
    }
    Ok(expected)
}

/// Remove a directory tree created by a test, ignoring errors.
#[cfg(test)]
pub(crate) fn remove_test_dir(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tor's own test vector (`src/test/test_hs_common.c`,
    /// `test_build_address`), whose key is RFC 8032 test 1.
    #[test]
    fn address_matches_tor_test_vector() {
        let seed: [u8; 32] = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
            0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
            0x1c, 0xae, 0x7f, 0x60,
        ];
        let pubkey: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        let address = "25njqamcweflpvkl73j4szahhihoc4xt3ktcgjnpaingr5yhkenl5sid.onion";
        assert_eq!(SecretKey::from_seed(&seed).public_key(), pubkey);
        assert_eq!(onion_address(&pubkey), address);
        assert_eq!(decode_onion_address(address), Ok(pubkey));
    }

    #[test]
    fn pubkey_chars_agree_with_full_address() {
        for _ in 0..200 {
            let secret = random_key();
            let pubkey = secret.public_key();
            assert_eq!(
                pubkey_chars(&pubkey)[..],
                address_chars(&pubkey)[..PUBKEY_CHARS]
            );
        }
    }

    #[test]
    fn address_structure() {
        for _ in 0..50 {
            let chars = address_chars(&random_key().public_key());
            // The version byte forces the last char to 'd' and the one before
            // it to one of a/i/q/y.
            assert_eq!(chars[55], b'd');
            assert!(b"aiqy".contains(&chars[54]));
        }
    }

    #[test]
    fn decode_rejects_bad_input() {
        let good = random_key().onion_address();
        assert!(decode_onion_address(&good.to_uppercase()).is_ok());
        assert_eq!(
            decode_onion_address("abc.onion"),
            Err(AddressError::Length(3))
        );
        let mut bad = good.clone().into_bytes();
        bad[3] = if bad[3] == b'a' { b'b' } else { b'a' };
        assert_eq!(
            decode_onion_address(std::str::from_utf8(&bad).unwrap()),
            Err(AddressError::Checksum)
        );
        let mut bad = good.into_bytes();
        bad[0] = b'1';
        assert_eq!(
            decode_onion_address(std::str::from_utf8(&bad).unwrap()),
            Err(AddressError::Character('1'))
        );
    }

    #[test]
    fn seed_expansion_matches_ed25519_dalek() {
        for _ in 0..20 {
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).unwrap();
            let ours = SecretKey::from_seed(&seed);
            let theirs = ed25519_dalek::SigningKey::from_bytes(&seed);
            assert_eq!(ours.public_key(), theirs.verifying_key().to_bytes());
            assert!(is_clamped(ours.scalar()));
            assert!(ours.self_test(&ours.public_key()));
        }
    }

    #[test]
    fn self_test_rejects_wrong_public_key() {
        let a = random_key();
        let b = random_key();
        assert!(!a.self_test(&b.public_key()));
    }

    #[test]
    fn writes_and_verifies_hidden_service_dir() {
        let dir = std::env::temp_dir().join(format!("torv3-vanity-test-{}", std::process::id()));
        remove_test_dir(&dir);
        let secret = random_key();
        let hs_dir = write_hidden_service_dir(&dir, &secret).unwrap();

        assert_eq!(
            verify_hidden_service_dir(&hs_dir).unwrap(),
            secret.onion_address()
        );
        let secret_file = fs::read(hs_dir.join("hs_ed25519_secret_key")).unwrap();
        assert_eq!(secret_file.len(), 96);
        assert_eq!(&secret_file[32..], secret.expanded());
        assert!(hs_dir.join("authorized_clients").is_dir());
        assert!(hs_dir.join("hostname").is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&hs_dir), 0o700);
            assert_eq!(mode(&hs_dir.join("hs_ed25519_secret_key")), 0o600);
        }

        // Never overwrite an existing key directory.
        assert!(write_hidden_service_dir(&dir, &secret).is_err());

        // Tampering is detected.
        fs::write(hs_dir.join("hostname"), "nope.onion\n").unwrap();
        assert!(matches!(
            verify_hidden_service_dir(&hs_dir),
            Err(KeyFileError::HostnameMismatch { .. })
        ));
        remove_test_dir(&dir);
    }

    fn random_key() -> SecretKey {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).unwrap();
        SecretKey::from_seed(&seed)
    }
}
