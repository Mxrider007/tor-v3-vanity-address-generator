//! Tor v3 onion address and hidden-service key utilities

use sha3::{Digest, Sha3_256};
use std::io;
use std::path::{Path, PathBuf};

/// Convert an Ed25519 public key to a Tor v3 onion address
///
/// The onion address format is: base32(pubkey || checksum || version).onion
/// where checksum = SHA3-256(".onion checksum" || pubkey || version)[0..2]
/// and version = 0x03
pub fn pubkey_to_onion(pubkey: &[u8; 32]) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(b".onion checksum");
    hasher.update(pubkey);
    hasher.update([3u8]);

    let mut onion = [0u8; 35];
    onion[..32].copy_from_slice(pubkey);
    onion[32..34].copy_from_slice(&hasher.finalize()[..2]);
    onion[34] = 3;

    format!(
        "{}.onion",
        base32::encode(base32::Alphabet::Rfc4648Lower { padding: false }, &onion)
    )
}

/// Derive the Tor v3 expanded Ed25519 secret key (`a || RH`) from a 32-byte seed.
///
/// This is the exact inner format Tor stores in `hs_ed25519_secret_key`: the
/// SHA-512 of the seed, with the lower half clamped to form the scalar `a`
/// (so that `A = a*B` is the public key) and the upper half used as the nonce
/// prefix `RH`. Tor signs with this expanded key directly, without re-clamping.
pub fn expanded_secret_key_from_seed(seed: &[u8; 32]) -> [u8; 64] {
    use sha2::{Digest as _, Sha512};
    let hash = Sha512::digest(seed);
    let mut expanded = [0u8; 64];
    expanded.copy_from_slice(&hash);
    // Standard Ed25519 scalar clamping.
    expanded[0] &= 248;
    expanded[31] &= 127;
    expanded[31] |= 64;
    expanded
}

/// Write a complete Tor v3 hidden-service directory for a found key.
///
/// Creates `<output_dir>/<onion-without-suffix>/` containing the four items a
/// Tor daemon needs to load a `HiddenServiceDir`:
/// - `hostname` — the `<addr>.onion` string
/// - `hs_ed25519_public_key` — 32-byte header + 32-byte public key (64 bytes)
/// - `hs_ed25519_secret_key` — 32-byte header + 64-byte expanded key (96 bytes)
/// - `authorized_clients/` — empty directory
///
/// On Unix, restrictive permissions are applied (dir `0700`, secret key `0600`)
/// so Tor does not refuse the key for being world-readable. Returns the path to
/// the created directory.
pub fn write_hidden_service_dir(
    output_dir: &Path,
    onion: &str,
    pubkey: &[u8; 32],
    expanded_secret: &[u8; 64],
) -> io::Result<PathBuf> {
    use std::io::Write;

    let dir_name = onion.trim_end_matches(".onion");
    let hs_dir = output_dir.join(dir_name);
    std::fs::create_dir_all(&hs_dir)?;

    // hostname
    let mut f = std::fs::File::create(hs_dir.join("hostname"))?;
    writeln!(f, "{}", onion)?;

    // hs_ed25519_public_key (32-byte header + 32-byte public key)
    let mut f = std::fs::File::create(hs_dir.join("hs_ed25519_public_key"))?;
    f.write_all(crate::PUBKEY_PREFIX)?;
    f.write_all(pubkey)?;

    // hs_ed25519_secret_key (32-byte header + 64-byte expanded key)
    let secret_path = hs_dir.join("hs_ed25519_secret_key");
    let mut f = std::fs::File::create(&secret_path)?;
    f.write_all(crate::FILE_PREFIX)?;
    f.write_all(expanded_secret)?;

    // authorized_clients/ (required by Tor, may be empty)
    std::fs::create_dir_all(hs_dir.join("authorized_clients"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hs_dir, std::fs::Permissions::from_mode(0o700))?;
        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(hs_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onion_format() {
        let pubkey = [0u8; 32];
        let onion = pubkey_to_onion(&pubkey);
        assert!(onion.ends_with(".onion"));
        assert_eq!(onion.len(), 56 + 6); // 56 base32 chars + ".onion"
    }

    #[test]
    fn test_expanded_key_matches_pubkey() {
        // The expanded key derived from a seed must reproduce the same public key
        // that ed25519_dalek derives from that seed — i.e. it is a valid Tor key.
        use ed25519_dalek::SigningKey;
        for _ in 0..32 {
            let seed: [u8; 32] = rand::random();
            let sk = SigningKey::from_bytes(&seed);
            let pk = sk.verifying_key();

            let expanded = expanded_secret_key_from_seed(&seed);
            let esk = ed25519_dalek::hazmat::ExpandedSecretKey::from_bytes(&expanded);
            let derived = ed25519_dalek::VerifyingKey::from(&esk);
            assert_eq!(derived.to_bytes(), pk.to_bytes());

            // And the expanded key signs messages that verify against that pubkey.
            use ed25519_dalek::Verifier;
            let sig = ed25519_dalek::hazmat::raw_sign::<sha2::Sha512>(&esk, b"msg", &pk);
            assert!(pk.verify(b"msg", &sig).is_ok());
        }
    }

    #[test]
    fn test_write_hidden_service_dir() {
        let dir = std::env::temp_dir().join(format!("kv-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let pubkey = [7u8; 32];
        let onion = pubkey_to_onion(&pubkey);
        let expanded = [9u8; 64];
        let hs_dir = write_hidden_service_dir(&dir, &onion, &pubkey, &expanded).unwrap();

        let pub_bytes = std::fs::read(hs_dir.join("hs_ed25519_public_key")).unwrap();
        let sec_bytes = std::fs::read(hs_dir.join("hs_ed25519_secret_key")).unwrap();
        let host = std::fs::read_to_string(hs_dir.join("hostname")).unwrap();
        assert_eq!(pub_bytes.len(), 64); // 32 header + 32 pubkey
        assert_eq!(sec_bytes.len(), 96); // 32 header + 64 expanded
        assert_eq!(&pub_bytes[32..], &pubkey[..]);
        assert_eq!(&sec_bytes[32..], &expanded[..]);
        assert_eq!(host.trim(), onion);
        assert!(hs_dir.join("authorized_clients").is_dir());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
