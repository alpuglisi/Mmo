//! Web Push message encryption (RFC 8291) in the `aes128gcm` content coding
//! (RFC 8188): one record, keyed from an ECDH agreement between a fresh
//! sender key pair and the subscription's P-256 key, salted, and mixed with
//! the subscription's authentication secret.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rand::RngCore;
use sha2::Sha256;

/// The record size written in the header. The whole message is one record,
/// so this only has to exceed it.
pub const RECORD_SIZE: u32 = 4096;
/// Header: salt (16), record size (4), key id length (1), key id (65).
const HEADER_LEN: usize = 16 + 4 + 1 + 65;
/// Largest plaintext that fits the record: the record size less the
/// AES-GCM tag (16) and the padding delimiter (1).
pub const MAX_PLAINTEXT: usize = RECORD_SIZE as usize - 16 - 1;

#[derive(Debug, PartialEq, Eq)]
pub enum EceError {
    /// The subscription's `p256dh` is not an uncompressed P-256 point.
    BadPublicKey,
    /// The authentication secret is not 16 bytes.
    BadAuthSecret,
    TooLong,
    /// Decryption: not a message for this key, or corrupt.
    Decrypt,
}

impl std::fmt::Display for EceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EceError::BadPublicKey => "the subscription key is not a P-256 public key",
            EceError::BadAuthSecret => "the subscription auth secret is not 16 bytes",
            EceError::TooLong => "the message is too long for one record",
            EceError::Decrypt => "the message does not decrypt",
        })
    }
}

impl std::error::Error for EceError {}

/// Check a subscription's keys: `p256dh` an uncompressed P-256 point (65
/// bytes), `auth` 16 bytes.
pub fn check_keys(ua_public: &[u8], auth: &[u8]) -> Result<(), EceError> {
    if ua_public.len() != 65 || PublicKey::from_sec1_bytes(ua_public).is_err() {
        return Err(EceError::BadPublicKey);
    }
    if auth.len() != 16 {
        return Err(EceError::BadAuthSecret);
    }
    Ok(())
}

/// The content-encryption key and nonce (RFC 8291 section 3.4).
fn derive(ecdh: &[u8], auth: &[u8], ua_public: &[u8], as_public: &[u8], salt: &[u8]) -> ([u8; 16], [u8; 12]) {
    let mut key_info = Vec::with_capacity(14 + 65 + 65);
    key_info.extend_from_slice(b"WebPush: info\0");
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), ecdh).expand(&key_info, &mut ikm).expect("32 bytes is a valid length");
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).expect("valid length");
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).expect("valid length");
    (cek, nonce)
}

/// Encrypt `plaintext` for a subscription (`ua_public`: its `p256dh` key,
/// `auth`: its auth secret) with a fresh sender key and salt.
pub fn encrypt(ua_public: &[u8], auth: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, EceError> {
    let sender = SecretKey::random(&mut rand::rngs::OsRng);
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    encrypt_with(ua_public, auth, plaintext, &sender, &salt)
}

/// [`encrypt`] with a given sender key and salt (the RFC's test vector).
pub fn encrypt_with(
    ua_public: &[u8],
    auth: &[u8],
    plaintext: &[u8],
    sender: &SecretKey,
    salt: &[u8; 16],
) -> Result<Vec<u8>, EceError> {
    check_keys(ua_public, auth)?;
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(EceError::TooLong);
    }
    let ua = PublicKey::from_sec1_bytes(ua_public).map_err(|_| EceError::BadPublicKey)?;
    let shared = p256::ecdh::diffie_hellman(sender.to_nonzero_scalar(), ua.as_affine());
    let as_public = sender.public_key().to_encoded_point(false);
    let as_public = as_public.as_bytes();
    let (cek, nonce) = derive(shared.raw_secret_bytes(), auth, ua_public, as_public, salt);
    // The only (so the last) record: the plaintext, then delimiter 2, no
    // padding.
    let mut record = Vec::with_capacity(plaintext.len() + 1);
    record.extend_from_slice(plaintext);
    record.push(2);
    let sealed = Aes128Gcm::new_from_slice(&cek)
        .expect("16-byte key")
        .encrypt(Nonce::from_slice(&nonce), record.as_slice())
        .map_err(|_| EceError::TooLong)?;
    let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    out.push(as_public.len() as u8);
    out.extend_from_slice(as_public);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Decrypt a one-record `aes128gcm` push message as the user agent would
/// (`ua_private`: the subscription's private key). For tests and tools: the
/// server only encrypts.
pub fn decrypt(ua_private: &SecretKey, auth: &[u8], body: &[u8]) -> Result<Vec<u8>, EceError> {
    if body.len() < 21 {
        return Err(EceError::Decrypt);
    }
    let salt = &body[..16];
    let idlen = body[20] as usize;
    let as_public = body.get(21..21 + idlen).ok_or(EceError::Decrypt)?;
    let sealed = &body[21 + idlen..];
    let sender = PublicKey::from_sec1_bytes(as_public).map_err(|_| EceError::Decrypt)?;
    let shared = p256::ecdh::diffie_hellman(ua_private.to_nonzero_scalar(), sender.as_affine());
    let ua_public = ua_private.public_key().to_encoded_point(false);
    let (cek, nonce) = derive(shared.raw_secret_bytes(), auth, ua_public.as_bytes(), as_public, salt);
    let mut record = Aes128Gcm::new_from_slice(&cek)
        .expect("16-byte key")
        .decrypt(Nonce::from_slice(&nonce), sealed)
        .map_err(|_| EceError::Decrypt)?;
    // Strip the padding back to the delimiter (2: the last record).
    while record.last() == Some(&0) {
        record.pop();
    }
    if record.pop() != Some(2) {
        return Err(EceError::Decrypt);
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webpush::{b64_decode, b64_encode};

    /// RFC 8291 section 5: the example message, byte for byte.
    #[test]
    fn rfc8291_test_vector() {
        let plaintext = b"When I grow up, I want to be a watermelon";
        let as_private = b64_decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw").unwrap();
        let as_public =
            b64_decode("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8")
                .unwrap();
        let ua_private = b64_decode("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94").unwrap();
        let ua_public =
            b64_decode("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4")
                .unwrap();
        let salt: [u8; 16] = b64_decode("DGv6ra1nlYgDCS1FRnbzlw").unwrap().try_into().unwrap();
        let auth = b64_decode("BTBZMqHH6r4Tts7J_aSIgg").unwrap();
        let expected = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";

        let sender = SecretKey::from_slice(&as_private).unwrap();
        assert_eq!(sender.public_key().to_encoded_point(false).as_bytes(), as_public.as_slice());
        let ua = SecretKey::from_slice(&ua_private).unwrap();
        assert_eq!(ua.public_key().to_encoded_point(false).as_bytes(), ua_public.as_slice());

        // The intermediate keys of the RFC's walk-through.
        let shared = p256::ecdh::diffie_hellman(sender.to_nonzero_scalar(), ua.public_key().as_affine());
        assert_eq!(b64_encode(shared.raw_secret_bytes()), "kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
        let (cek, nonce) = derive(shared.raw_secret_bytes(), &auth, &ua_public, &as_public, &salt);
        assert_eq!(b64_encode(&cek), "oIhVW04MRdy2XN9CiKLxTg");
        assert_eq!(b64_encode(&nonce), "4h_95klXJ5E_qnoN");

        let body = encrypt_with(&ua_public, &auth, plaintext, &sender, &salt).unwrap();
        assert_eq!(b64_encode(&body), expected);
        assert_eq!(decrypt(&ua, &auth, &body).unwrap(), plaintext);
    }

    #[test]
    fn random_keys_round_trip_and_bad_input_is_refused() {
        let ua = SecretKey::random(&mut rand::rngs::OsRng);
        let ua_public = ua.public_key().to_encoded_point(false);
        let auth = [7u8; 16];
        let msg = vec![b'x'; MAX_PLAINTEXT];
        let a = encrypt(ua_public.as_bytes(), &auth, &msg).unwrap();
        let b = encrypt(ua_public.as_bytes(), &auth, &msg).unwrap();
        // A fresh salt and sender key every time.
        assert_ne!(a[..86], b[..86]);
        assert!(a.len() <= HEADER_LEN + RECORD_SIZE as usize);
        assert_eq!(decrypt(&ua, &auth, &a).unwrap(), msg);
        assert_eq!(decrypt(&ua, &[8u8; 16], &a), Err(EceError::Decrypt));
        assert_eq!(encrypt(ua_public.as_bytes(), &auth, &[0; MAX_PLAINTEXT + 1]), Err(EceError::TooLong));
        assert_eq!(encrypt(&ua_public.as_bytes()[..33], &auth, b"x"), Err(EceError::BadPublicKey));
        assert_eq!(encrypt(&[4u8; 65], &auth, b"x"), Err(EceError::BadPublicKey));
        assert_eq!(encrypt(ua_public.as_bytes(), &auth[..8], b"x"), Err(EceError::BadAuthSecret));
    }
}
