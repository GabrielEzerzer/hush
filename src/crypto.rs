//! Sealing and opening vault files with XChaCha20-Poly1305: `HUSH1`, a 24-byte random nonce, then ciphertext and tag.
//! The additional data names the project and environment, so a file copied over another one fails to open.

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

pub fn key_id(key: &MasterKey) -> String {
    let mut h = Sha256::new();
    h.update(b"hush key id v1");
    h.update(&key[..]);
    hex::encode(&h.finalize()[..8])
}

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

    use proptest::prelude::*;
    use proptest::test_runner::RngSeed;

    fn seeded() -> ProptestConfig {
        ProptestConfig {
            cases: 64,
            rng_seed: RngSeed::Fixed(1),
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    fn key_from(bytes: [u8; KEY_LEN]) -> MasterKey {
        Zeroizing::new(bytes)
    }

    proptest! {
        #![proptest_config(seeded())]

        #[test]
        fn seal_then_open_returns_the_plaintext(
            key in any::<[u8; KEY_LEN]>(),
            aad in ".{0,40}",
            plaintext in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let key = key_from(key);
            let sealed = seal(&key, &aad, &plaintext).unwrap();
            prop_assert_eq!(&open(&key, &aad, &sealed).unwrap()[..], &plaintext[..]);
        }

        #[test]
        fn any_flipped_bit_or_cut_byte_fails_to_open(
            key in any::<[u8; KEY_LEN]>(),
            plaintext in proptest::collection::vec(any::<u8>(), 0..64),
            at in any::<proptest::sample::Index>(),
            bit in 0u8..8,
        ) {
            let key = key_from(key);
            let sealed = seal(&key, "hush/v1/a/dev", &plaintext).unwrap();
            let mut flipped = sealed.clone();
            flipped[at.index(sealed.len())] ^= 1 << bit;
            prop_assert!(open(&key, "hush/v1/a/dev", &flipped).is_err());
            prop_assert!(open(&key, "hush/v1/a/dev", &sealed[..at.index(sealed.len())]).is_err());
        }

        #[test]
        fn another_project_environment_or_key_fails_to_open(
            key in any::<[u8; KEY_LEN]>(),
            other_key in any::<[u8; KEY_LEN]>(),
            aad in "[a-z0-9/-]{1,30}",
            other_aad in "[a-z0-9/-]{1,30}",
        ) {
            prop_assume!(aad != other_aad && key != other_key);
            let sealed = seal(&key_from(key), &aad, b"value").unwrap();
            prop_assert!(open(&key_from(key), &other_aad, &sealed).is_err());
            prop_assert!(open(&key_from(other_key), &aad, &sealed).is_err());
        }

        #[test]
        fn the_recovery_form_decodes_to_the_same_key(key in any::<[u8; KEY_LEN]>()) {
            let key = key_from(key);
            let encoded = encode_key(&key);
            prop_assert_eq!(encoded.len(), 64 + 7);
            prop_assert_eq!(&decode_key(&encoded).unwrap()[..], &key[..]);
            let retyped = encoded.replace('-', " ").to_uppercase();
            prop_assert_eq!(&decode_key(&retyped).unwrap()[..], &key[..]);
            prop_assert_eq!(key_id(&key), key_id(&decode_key(&encoded).unwrap()));
        }
    }
}
