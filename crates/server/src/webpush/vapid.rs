//! VAPID (RFC 8292): the server's P-256 key pair, which browsers bind a
//! subscription to (`applicationServerKey`), and the signed JWT that
//! authenticates each request to a push service.

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;

use super::{b64_decode, b64_encode};

/// The server's VAPID key pair.
#[derive(Clone)]
pub struct VapidKey {
    secret: SecretKey,
    /// The uncompressed public key, base64url (the browser's
    /// `applicationServerKey` and the `k=` of the authorization header).
    public: String,
}

impl std::fmt::Debug for VapidKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VapidKey({})", self.public)
    }
}

impl VapidKey {
    pub fn generate() -> Self {
        Self::from_secret(SecretKey::random(&mut rand::rngs::OsRng))
    }

    fn from_secret(secret: SecretKey) -> Self {
        let public = b64_encode(secret.public_key().to_encoded_point(false).as_bytes());
        VapidKey { secret, public }
    }

    /// From the private key as base64url (the raw 32-byte scalar, as
    /// [`VapidKey::private_b64`] writes it and most VAPID tools print it).
    pub fn from_private_b64(s: &str) -> Result<Self, String> {
        let bytes = b64_decode(s.trim()).ok_or("the VAPID private key is not base64url")?;
        if bytes.len() != 32 {
            return Err(format!("the VAPID private key is {} bytes, not 32", bytes.len()));
        }
        let secret = SecretKey::from_slice(&bytes).map_err(|_| "the VAPID private key is not a P-256 scalar")?;
        Ok(Self::from_secret(secret))
    }

    pub fn private_b64(&self) -> String {
        b64_encode(&self.secret.to_bytes())
    }

    pub fn public_b64(&self) -> &str {
        &self.public
    }

    /// A signed JWT for the push service at `audience` (the endpoint's
    /// origin), valid until `exp` (Unix seconds), naming `subject` (a
    /// `mailto:` or `https:` contact for the push service's operators).
    pub fn jwt(&self, audience: &str, exp: u64, subject: &str) -> String {
        let header = b64_encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({ "aud": audience, "exp": exp, "sub": subject });
        let claims = b64_encode(claims.to_string().as_bytes());
        let signing_input = format!("{header}.{claims}");
        let sig: Signature = SigningKey::from(&self.secret).sign(signing_input.as_bytes());
        format!("{signing_input}.{}", b64_encode(&sig.to_bytes()))
    }

    /// The `Authorization` header value (`vapid t=<jwt>, k=<public key>`).
    pub fn authorization(&self, audience: &str, exp: u64, subject: &str) -> String {
        format!("vapid t={}, k={}", self.jwt(audience, exp, subject), self.public)
    }
}

/// Check a VAPID authorization header against a public key (base64url):
/// the signature verifies and the claims parse. Returns the claims. For
/// tests (the mock push service).
pub fn verify(authorization: &str, public_b64: &str) -> Result<serde_json::Value, String> {
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;
    let rest = authorization.strip_prefix("vapid ").ok_or("not a vapid authorization")?;
    let mut token = None;
    let mut key = None;
    for part in rest.split(',') {
        let part = part.trim();
        if let Some(t) = part.strip_prefix("t=") {
            token = Some(t);
        } else if let Some(k) = part.strip_prefix("k=") {
            key = Some(k);
        }
    }
    let (token, key) = (token.ok_or("no t=")?, key.ok_or("no k=")?);
    if key != public_b64 {
        return Err("k= is another key".into());
    }
    let (input, sig) = token.rsplit_once('.').ok_or("malformed token")?;
    let sig = Signature::from_slice(&b64_decode(sig).ok_or("signature not base64url")?).map_err(|e| e.to_string())?;
    let vk = VerifyingKey::from_sec1_bytes(&b64_decode(key).ok_or("key not base64url")?).map_err(|e| e.to_string())?;
    vk.verify(input.as_bytes(), &sig).map_err(|e| e.to_string())?;
    let claims = input.split('.').nth(1).ok_or("malformed token")?;
    serde_json::from_slice(&b64_decode(claims).ok_or("claims not base64url")?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_tokens_verify() {
        let k = VapidKey::generate();
        let again = VapidKey::from_private_b64(&k.private_b64()).unwrap();
        assert_eq!(again.public_b64(), k.public_b64());
        assert_eq!(b64_decode(k.public_b64()).unwrap().len(), 65);
        let auth = k.authorization("https://push.example.net", 1_900_000_000, "mailto:ops@example.org");
        let claims = verify(&auth, k.public_b64()).unwrap();
        assert_eq!(claims["aud"], "https://push.example.net");
        assert_eq!(claims["exp"], 1_900_000_000u64);
        assert_eq!(claims["sub"], "mailto:ops@example.org");
        // Another key's token doesn't verify.
        let other = VapidKey::generate();
        assert!(verify(&other.authorization("https://a", 1, "mailto:x@y"), k.public_b64()).is_err());
        assert!(VapidKey::from_private_b64("short").is_err());
        assert!(VapidKey::from_private_b64(&b64_encode(&[0u8; 32])).is_err());
    }
}
