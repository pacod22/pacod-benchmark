//! The admin tokens of gateways added through the UI, sealed in the database under the
//! key-encryption key (`BENCH_KEK`), which lives outside it.
//!
//! Copied from pacod-id, `crates/pacod-id/src/crypto/envelope.rs` and the two helpers of its
//! `crypto/mod.rs` it uses, at commit 3872f3261e3e (vendor/pacod-id/VERSION); `fingerprint` is
//! pacod-gw's. A copy rather than a dependency: the `pacod-id` service crate would pull the
//! whole identity provider into this one, and pacod-id is built once only (`make vendor-check`).
//! A change there is copied here by hand.
//!
//! Each secret is encrypted with its own data key (AES-256-GCM). The data key is encrypted
//! with the key-encryption key. The record carries the id of the key-encryption key so that it
//! can be rotated (`pacod-bench rewrap-secrets`).

use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::config::{Env, secret};

/// Fills a buffer from the operating system's random source.
fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    SystemRandom::new().fill(&mut b).expect("the system random source is available");
    b
}

/// A key check value: the first eight bytes of an HMAC over a fixed text, keyed with the
/// secret. It tells two keys apart without revealing either.
fn check_value(secret: &[u8]) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hex::encode(&hmac::sign(&key, b"pacod-bench key check").as_ref()[..8])
}

/// The key-encryption key, with its id.
#[derive(Clone)]
pub struct Kek {
    pub id: String,
    key: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for Kek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kek").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Kek {
    pub fn new(id: impl Into<String>, key: [u8; 32]) -> Self {
        Self { id: id.into(), key: Zeroizing::new(key) }
    }

    /// See [`check_value`].
    pub fn check_value(&self) -> String {
        check_value(self.key.as_slice())
    }

    /// Parses a base64-encoded 32-byte key.
    pub fn from_base64(id: impl Into<String>, encoded: &str) -> anyhow::Result<Self> {
        let raw = Zeroizing::new(B64.decode(encoded.trim())?);
        let key: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("the key-encryption key must be 32 bytes, base64-encoded"))?;
        Ok(Self::new(id, key))
    }
}

/// An encrypted secret as stored in the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    pub kek_id: String,
    /// The data key, encrypted with the key-encryption key (nonce prepended), base64.
    pub wrapped_dek: String,
    /// The secret, encrypted with the data key (nonce prepended), base64.
    pub ciphertext: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("the secret was encrypted with key-encryption key \"{0}\", which is not configured")]
    UnknownKek(String),
    #[error("the stored secret is malformed or was encrypted with a different key")]
    Decrypt,
}

fn seal_with(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    let nonce_bytes: [u8; 12] = random_bytes();
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).expect("32-byte key"));
    let mut buf = plaintext.to_vec();
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce_bytes), Aad::from(aad), &mut buf)
        .expect("sealing in memory cannot fail");
    let mut out = Vec::with_capacity(12 + buf.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&buf);
    out
}

fn open_with(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, EnvelopeError> {
    if sealed.len() < 12 + AES_256_GCM.tag_len() {
        return Err(EnvelopeError::Decrypt);
    }
    let (nonce, ct) = sealed.split_at(12);
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| EnvelopeError::Decrypt)?;
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|_| EnvelopeError::Decrypt)?);
    let mut buf = Zeroizing::new(ct.to_vec());
    let len = key
        .open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(aad), &mut buf)
        .map_err(|_| EnvelopeError::Decrypt)?
        .len();
    buf.truncate(len);
    Ok(buf)
}

/// Encrypts `plaintext`. `context` binds the ciphertext to its record, so a ciphertext copied
/// onto another record does not decrypt.
pub fn seal(kek: &Kek, context: &str, plaintext: &[u8]) -> Sealed {
    let dek: Zeroizing<[u8; 32]> = Zeroizing::new(random_bytes());
    Sealed {
        kek_id: kek.id.clone(),
        wrapped_dek: B64.encode(seal_with(&kek.key, context.as_bytes(), dek.as_slice())),
        ciphertext: B64.encode(seal_with(&dek, context.as_bytes(), plaintext)),
    }
}

/// Decrypts a secret. `keks` holds the current key-encryption key and any older ones.
pub fn open(keks: &[Kek], context: &str, sealed: &Sealed) -> Result<Zeroizing<Vec<u8>>, EnvelopeError> {
    let kek =
        keks.iter().find(|k| k.id == sealed.kek_id).ok_or_else(|| EnvelopeError::UnknownKek(sealed.kek_id.clone()))?;
    let wrapped = B64.decode(&sealed.wrapped_dek).map_err(|_| EnvelopeError::Decrypt)?;
    let dek_bytes = open_with(&kek.key, context.as_bytes(), &wrapped)?;
    let dek: [u8; 32] = dek_bytes.as_slice().try_into().map_err(|_| EnvelopeError::Decrypt)?;
    let dek = Zeroizing::new(dek);
    let ct = B64.decode(&sealed.ciphertext).map_err(|_| EnvelopeError::Decrypt)?;
    open_with(&dek, context.as_bytes(), &ct)
}

/// Wraps the data key of `sealed` under `current`, after a rotation of the key-encryption key.
/// The secret's own ciphertext stays as it is. `None` when it is already under `current`.
pub fn rewrap(keks: &[Kek], current: &Kek, context: &str, sealed: &Sealed) -> Result<Option<Sealed>, EnvelopeError> {
    if sealed.kek_id == current.id {
        return Ok(None);
    }
    let kek =
        keks.iter().find(|k| k.id == sealed.kek_id).ok_or_else(|| EnvelopeError::UnknownKek(sealed.kek_id.clone()))?;
    let wrapped = B64.decode(&sealed.wrapped_dek).map_err(|_| EnvelopeError::Decrypt)?;
    let dek = open_with(&kek.key, context.as_bytes(), &wrapped)?;
    Ok(Some(Sealed {
        kek_id: current.id.clone(),
        wrapped_dek: B64.encode(seal_with(&current.key, context.as_bytes(), &dek)),
        ciphertext: sealed.ciphertext.clone(),
    }))
}

/// A short, non-reversible label for a secret, shown instead of the secret: the last four
/// characters and the first eight hex digits of its SHA-256.
pub fn fingerprint(secret: &str) -> String {
    let tail: String = {
        let chars: Vec<char> = secret.chars().collect();
        chars[chars.len().saturating_sub(4)..].iter().collect()
    };
    let digest = ring::digest::digest(&ring::digest::SHA256, secret.as_bytes());
    format!("…{tail} ({})", hex::encode(&digest.as_ref()[..4]))
}

/// The key-encryption keys: the current one first, then older ones that still open records
/// sealed before a rotation.
#[derive(Clone, Debug)]
pub struct Keys(Vec<Kek>);

impl Keys {
    /// `BENCH_KEK` (base64 of 32 bytes), `BENCH_KEK_ID` (default `default`), `BENCH_KEK_OLD`
    /// (`id=base64,…`; in a file, one per line also works), each of them also from `_FILE`.
    /// None without `BENCH_KEK`: then no gateway is added through the UI.
    pub fn from_env(env: Env) -> anyhow::Result<Option<Keys>> {
        let Some(current) = secret(env, "BENCH_KEK")? else {
            anyhow::ensure!(secret(env, "BENCH_KEK_OLD")?.is_none(), "BENCH_KEK_OLD needs BENCH_KEK");
            return Ok(None);
        };
        let id = env("BENCH_KEK_ID").map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let mut keks = vec![Kek::from_base64(id.unwrap_or_else(|| "default".into()), &current).context("BENCH_KEK")?];
        if let Some(old) = secret(env, "BENCH_KEK_OLD")? {
            for pair in old.split([',', '\n']).filter(|p| !p.trim().is_empty()) {
                let (id, key) = pair.trim().split_once('=').context("BENCH_KEK_OLD entries look like id=base64key")?;
                let kek = Kek::from_base64(id.trim(), key).with_context(|| format!("BENCH_KEK_OLD entry {id}"))?;
                anyhow::ensure!(keks.iter().all(|k| k.id != kek.id), "the key id {id} is given twice");
                keks.push(kek);
            }
        }
        Ok(Some(Keys(keks)))
    }

    pub fn new(current: Kek, older: Vec<Kek>) -> Keys {
        let mut keks = vec![current];
        keks.extend(older);
        Keys(keks)
    }

    pub fn current(&self) -> &Kek {
        &self.0[0]
    }

    pub fn all(&self) -> &[Kek] {
        &self.0
    }
}

/// What a gateway's sealed token is bound to: its target and its management API. A token copied
/// onto another target, or kept while the address changes, does not open.
pub fn token_context(target_id: &str, admin_url: &str) -> String {
    format!("{target_id}\n{}", admin_url.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn kek(id: &str, byte: u8) -> Kek {
        Kek::new(id, [byte; 32])
    }

    #[test]
    fn round_trip() {
        let k = kek("k1", 7);
        let sealed = seal(&k, "tgt_1", b"pgwa_secret");
        assert_eq!(&open(&[k], "tgt_1", &sealed).unwrap()[..], b"pgwa_secret");
        assert!(!sealed.ciphertext.contains("pgwa"));
    }

    #[test]
    fn same_plaintext_encrypts_differently() {
        let k = kek("k1", 7);
        let a = seal(&k, "c", b"x");
        let b = seal(&k, "c", b"x");
        assert_ne!(a.ciphertext, b.ciphertext);
        assert_ne!(a.wrapped_dek, b.wrapped_dek);
    }

    #[test]
    fn a_token_is_bound_to_its_target_and_its_address() {
        let k = kek("k1", 7);
        let here = token_context("tgt_1", "http://gw-admin:8081/");
        let sealed = seal(&k, &here, b"pgwa_secret");
        let keys = std::slice::from_ref(&k);
        assert!(open(keys, &token_context("tgt_1", "http://gw-admin:8081"), &sealed).is_ok(), "a slash is no change");
        assert!(matches!(
            open(keys, &token_context("tgt_2", "http://gw-admin:8081"), &sealed),
            Err(EnvelopeError::Decrypt)
        ));
        assert!(matches!(
            open(keys, &token_context("tgt_1", "http://evil:8081"), &sealed),
            Err(EnvelopeError::Decrypt)
        ));
    }

    #[test]
    fn wrong_key_or_tampering_fails() {
        let k = kek("k1", 7);
        let sealed = seal(&k, "c", b"secret");
        assert!(matches!(open(&[kek("k1", 8)], "c", &sealed), Err(EnvelopeError::Decrypt)));
        assert!(matches!(open(&[kek("k2", 7)], "c", &sealed), Err(EnvelopeError::UnknownKek(_))));
        let mut bad = sealed.clone();
        let mut raw = B64.decode(&bad.ciphertext).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        bad.ciphertext = B64.encode(raw);
        assert!(matches!(open(&[k], "c", &bad), Err(EnvelopeError::Decrypt)));
    }

    #[test]
    fn rewrapping_moves_the_data_key_to_the_current_key() {
        let (old, new) = (kek("2026-01", 1), kek("2027-01", 2));
        let sealed = seal(&old, "c", b"pgwa_secret");
        let moved = rewrap(&[new.clone(), old.clone()], &new, "c", &sealed).unwrap().unwrap();
        assert_eq!(moved.kek_id, "2027-01");
        assert_eq!(moved.ciphertext, sealed.ciphertext, "the secret itself is not encrypted again");
        assert_eq!(open(std::slice::from_ref(&new), "c", &moved).unwrap().as_slice(), b"pgwa_secret");
        assert!(rewrap(std::slice::from_ref(&new), &new, "c", &moved).unwrap().is_none(), "already current");
        assert!(rewrap(&[new.clone(), old], &new, "other", &sealed).is_err(), "bound to its record");
    }

    #[test]
    fn keys_come_from_the_environment_with_older_ones_after_the_current() {
        let b64 = |b: u8| B64.encode([b; 32]);
        let vars: HashMap<String, String> = [
            ("BENCH_KEK".to_owned(), b64(2)),
            ("BENCH_KEK_ID".to_owned(), "2026-10".to_owned()),
            ("BENCH_KEK_OLD".to_owned(), format!("2026-01={}", b64(1))),
        ]
        .into_iter()
        .collect();
        let env = |k: &str| vars.get(k).cloned();
        let keys = Keys::from_env(&env).unwrap().unwrap();
        assert_eq!(keys.current().id, "2026-10");
        assert_eq!(keys.all().iter().map(|k| k.id.as_str()).collect::<Vec<_>>(), ["2026-10", "2026-01"]);
        let sealed = seal(&kek("2026-01", 1), "c", b"x");
        assert!(open(keys.all(), "c", &sealed).is_ok(), "an old key still opens its records");

        let none = |_: &str| -> Option<String> { None };
        assert!(Keys::from_env(&none).unwrap().is_none(), "no key: no sealed tokens");
        let short = |k: &str| (k == "BENCH_KEK").then(|| B64.encode([0u8; 16]));
        assert!(Keys::from_env(&short).is_err());
    }

    #[test]
    fn a_fingerprint_shows_the_end_and_a_digest() {
        let f = fingerprint("pgwa_0123456789abcdef");
        assert!(f.starts_with("…cdef ("), "{f}");
        assert!(!f.contains("pgwa_0123"));
    }
}
