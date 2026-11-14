//! What the native senders share: device token checks, PEM keys and
//! fitting a notification into the services' payload limits.
//!
//! The Android and iOS apps (Capacitor, `@capacitor/push-notifications`)
//! register with their platform's push service and hand the device token to
//! the server (`POST /api/push/native/subscribe`). The service task then
//! sends each coalesced notification to those tokens next to the Web Push
//! subscriptions: through Firebase Cloud Messaging's HTTP v1 API for
//! Android ([`super::fcm`]) and Apple's token-based provider API for iOS
//! ([`super::apns`]). Either is on only when its credentials are configured.

use crate::protocol::NativeService;

use super::service::Payload;

/// Longest FCM registration token accepted (they are about 160 bytes).
pub const MAX_FCM_TOKEN: usize = 4096;
/// APNs device tokens are 32 bytes today (64 hex digits); Apple says they
/// may grow, so up to 100 bytes is accepted.
pub const MAX_APNS_TOKEN: usize = 200;
/// Room left for the notification body in a message: both services cap the
/// payload at 4 KB, and the title, data and JSON around it need some.
pub const MAX_BODY_BYTES: usize = 3000;

/// Check (and normalize) a device token for `service`: an FCM token is
/// URL-safe text, an APNs token hex (lower-cased).
pub fn check_token(service: NativeService, token: &str) -> Result<String, String> {
    let token = token.trim();
    match service {
        NativeService::Fcm => {
            let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.');
            if token.is_empty() || token.len() > MAX_FCM_TOKEN || !token.chars().all(ok) {
                return Err("not an FCM registration token".into());
            }
            Ok(token.to_string())
        }
        NativeService::Apns => {
            let hex = token.len().is_multiple_of(2) && token.chars().all(|c| c.is_ascii_hexdigit());
            if token.len() < 32 || token.len() > MAX_APNS_TOKEN || !hex {
                return Err("not an APNs device token (hex)".into());
            }
            Ok(token.to_ascii_lowercase())
        }
    }
}

/// The DER bytes of the first PEM block labelled `label` (`PRIVATE KEY`
/// for PKCS#8). Literal `\n` sequences count as line breaks, for keys
/// pasted into one-line environment variables.
pub fn pem_der(pem: &str, label: &str) -> Result<Vec<u8>, String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let pem = pem.replace("\\n", "\n");
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem.find(&begin).ok_or_else(|| format!("no {begin} block"))? + begin.len();
    let stop = pem[start..].find(&end).ok_or_else(|| format!("no {end} line"))? + start;
    let b64: String = pem[start..stop].chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(b64).map_err(|e| format!("the {label} block is not base64: {e}"))
}

/// `s` cut to at most `max` bytes on a character boundary, with an ellipsis
/// when cut.
pub fn fit(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// The custom data a native notification carries, so a tap opens its world
/// and tab (the same fields a Web Push payload has, as strings: FCM data
/// values must be strings).
pub fn data(p: &Payload) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert("world".into(), p.world.clone().into());
    m.insert("tab".into(), p.tab.clone().into());
    m.insert("kind".into(), p.kind.name().into());
    m.insert("count".into(), p.count.to_string().into());
    m.insert("tag".into(), p.tag.clone().into());
    if let Some(o) = p.organism {
        m.insert("organism".into(), o.to_string().into());
    }
    m
}

/// Mating offers and deaths are urgent; the rest can wait for a phone that
/// saves power.
pub fn urgent(p: &Payload) -> bool {
    matches!(p.kind, crate::protocol::PushKind::Mating | crate::protocol::PushKind::Death)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_checked_per_service() {
        let fcm = "dGVzdA:APA91bH-x_y.z";
        assert_eq!(check_token(NativeService::Fcm, &format!(" {fcm} ")), Ok(fcm.to_string()));
        assert!(check_token(NativeService::Fcm, "").is_err());
        assert!(check_token(NativeService::Fcm, "a b").is_err());
        assert!(check_token(NativeService::Fcm, "a/b").is_err());
        assert!(check_token(NativeService::Fcm, &"a".repeat(MAX_FCM_TOKEN + 1)).is_err());
        let apns = "AB".repeat(32);
        assert_eq!(check_token(NativeService::Apns, &apns), Ok("ab".repeat(32)));
        assert!(check_token(NativeService::Apns, "abc").is_err());
        assert!(check_token(NativeService::Apns, &"g".repeat(64)).is_err());
        assert!(check_token(NativeService::Apns, &"a".repeat(65)).is_err());
        assert!(check_token(NativeService::Apns, &"a".repeat(MAX_APNS_TOKEN + 2)).is_err());
    }

    #[test]
    fn pem_blocks_decode_with_real_or_escaped_line_breaks() {
        let pem = "junk\n-----BEGIN PRIVATE KEY-----\nAAEC\nAw==\n-----END PRIVATE KEY-----\n";
        assert_eq!(pem_der(pem, "PRIVATE KEY").unwrap(), vec![0, 1, 2, 3]);
        assert_eq!(pem_der(&pem.replace('\n', "\\n"), "PRIVATE KEY").unwrap(), vec![0, 1, 2, 3]);
        assert!(pem_der(pem, "EC PRIVATE KEY").is_err());
        assert!(pem_der("-----BEGIN PRIVATE KEY-----\n!!\n-----END PRIVATE KEY-----", "PRIVATE KEY").is_err());
    }

    #[test]
    fn fit_cuts_on_character_boundaries() {
        assert_eq!(fit("short", 10), "short");
        let cut = fit(&"é".repeat(100), 11);
        assert!(cut.len() <= 11 && cut.ends_with('…'), "{cut}");
        assert_eq!(cut, format!("{}…", "é".repeat(4)));
    }
}
