//! Sealing and opening vault files with XChaCha20-Poly1305.
//!
//! File layout: `HUSH1` magic, a 24-byte random nonce, then the ciphertext with its tag.
//! The additional data names the project and environment, so a file copied over another
//! project's or environment's file fails to open instead of silently serving the wrong secrets.

use anyhow::{Context, Result, bail, ensure};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const KEY_LEN: usize = 32;
pub type MasterKey = Zeroizing<[u8; KEY_LEN]>;

const MAGIC: &[u8] = b"HUSH1";
const NONCE_LEN: usize = 24;

pub fn new_key() -> MasterKey {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    OsRng.fill_bytes(&mut key[..]);
    key
}

pub fn seal(key: &MasterKey, aad: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key[..].into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(key: &MasterKey, aad: &str, data: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    ensure!(
        data.len() >= MAGIC.len() + NONCE_LEN && data.starts_with(MAGIC),
        "not a hush vault file"
    );
    let (nonce, ct) = data[MAGIC.len()..].split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key[..].into());
    match cipher.decrypt(
        XNonce::from_slice(nonce),
        Payload {
            msg: ct,
            aad: aad.as_bytes(),
        },
    ) {
        Ok(pt) => Ok(Zeroizing::new(pt)),
        Err(_) => bail!(
            "cannot decrypt: the file was modified, moved from another project or environment, or the key is wrong"
        ),
    }
}

/// A short public fingerprint of the key, stored in vault.toml so a wrong key fails with a
/// clear message before any file is touched.
pub fn key_id(key: &MasterKey) -> String {
    let mut h = Sha256::new();
    h.update(b"hush key id v1");
    h.update(&key[..]);
    hex::encode(&h.finalize()[..8])
}

/// The recovery form of the key: 64 hex digits in groups of eight.
pub fn encode_key(key: &MasterKey) -> Zeroizing<String> {
    let hex = Zeroizing::new(hex::encode(&key[..]));
    let groups: Vec<&str> = (0..8).map(|i| &hex[i * 8..i * 8 + 8]).collect();
    Zeroizing::new(groups.join("-"))
}

pub fn decode_key(s: &str) -> Result<MasterKey> {
    let compact = Zeroizing::new(
        s.chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect::<String>(),
    );
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    hex::decode_to_slice(compact.as_bytes(), &mut key[..])
        .context("a hush key is 64 hex digits (dashes and spaces are ignored)")?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let k = new_key();
        let sealed = seal(&k, "hush/v1/a/dev", b"hello").unwrap();
        assert_eq!(&open(&k, "hush/v1/a/dev", &sealed).unwrap()[..], b"hello");
    }

    #[test]
    fn nonces_differ() {
        let k = new_key();
        assert_ne!(seal(&k, "x", b"same").unwrap(), seal(&k, "x", b"same").unwrap());
    }

    #[test]
    fn wrong_aad_wrong_key_and_tampering_fail() {
        let k = new_key();
        let sealed = seal(&k, "hush/v1/a/dev", b"hello").unwrap();
        assert!(open(&k, "hush/v1/a/prod", &sealed).is_err());
        assert!(open(&k, "hush/v1/b/dev", &sealed).is_err());
        assert!(open(&new_key(), "hush/v1/a/dev", &sealed).is_err());
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 1;
            assert!(
                open(&k, "hush/v1/a/dev", &bad).is_err(),
                "flip at byte {i} went undetected"
            );
        }
        assert!(open(&k, "hush/v1/a/dev", &sealed[..sealed.len() - 1]).is_err());
    }

    #[test]
    fn key_encoding_round_trips() {
        let k = new_key();
        let enc = encode_key(&k);
        assert_eq!(enc.len(), 64 + 7);
        assert_eq!(&decode_key(&enc).unwrap()[..], &k[..]);
        assert_eq!(&decode_key(&enc.replace('-', " ").to_uppercase()).unwrap()[..], &k[..]);
        assert!(decode_key("abcd").is_err());
        assert_ne!(key_id(&k), key_id(&new_key()));
    }
}
