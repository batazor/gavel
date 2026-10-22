use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use url::Url;

use super::{ApplicationError, DerivedPrincipal};

const MAX_ISSUER_BYTES: usize = 2048;
const MAX_SUBJECT_BYTES: usize = 1024;

#[derive(Clone)]
pub struct PrincipalDeriver {
    key: [u8; 32],
}

impl fmt::Debug for PrincipalDeriver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrincipalDeriver")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl PrincipalDeriver {
    pub const fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub fn derive(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<DerivedPrincipal, ApplicationError> {
        let issuer = normalize_issuer(issuer)?;
        validate_subject(subject)?;

        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            .expect("HMAC-SHA256 accepts a key of any size");
        update_length_prefixed(&mut mac, issuer.as_bytes());
        update_length_prefixed(&mut mac, subject.as_bytes());
        let digest = mac.finalize().into_bytes();
        Ok(DerivedPrincipal {
            principal_id: format!("u1_{}", URL_SAFE_NO_PAD.encode(digest)),
        })
    }
}

fn normalize_issuer(raw: &str) -> Result<String, ApplicationError> {
    if raw.is_empty() || raw.len() > MAX_ISSUER_BYTES || raw.chars().any(char::is_control) {
        return Err(invalid("invalid_issuer", "issuer is invalid"));
    }
    let mut issuer = Url::parse(raw).map_err(|_| invalid("invalid_issuer", "issuer is invalid"))?;
    if issuer.scheme() != "https"
        || issuer.host_str().is_none()
        || !issuer.username().is_empty()
        || issuer.password().is_some()
        || issuer.query().is_some()
        || issuer.fragment().is_some()
    {
        return Err(invalid("invalid_issuer", "issuer is invalid"));
    }
    if issuer.path() != "/" {
        let normalized_path = issuer.path().trim_end_matches('/').to_owned();
        issuer.set_path(&normalized_path);
    }
    Ok(issuer.to_string().trim_end_matches('/').to_owned())
}

fn validate_subject(subject: &str) -> Result<(), ApplicationError> {
    if subject.is_empty()
        || subject.len() > MAX_SUBJECT_BYTES
        || subject.chars().any(char::is_control)
    {
        Err(invalid("invalid_subject", "subject is invalid"))
    } else {
        Ok(())
    }
}

fn update_length_prefixed(mac: &mut Hmac<Sha256>, value: &[u8]) {
    mac.update(&(value.len() as u64).to_be_bytes());
    mac.update(value);
}

fn invalid(reason: &'static str, safe_message: &'static str) -> ApplicationError {
    ApplicationError::InvalidArgument {
        reason,
        safe_message: safe_message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deriver() -> PrincipalDeriver {
        PrincipalDeriver::new([7; 32])
    }

    #[test]
    fn derivation_is_deterministic_normalized_and_opaque() {
        let first = deriver()
            .derive("https://ID.Example.test/oidc/", "external-user-42")
            .unwrap();
        let second = deriver()
            .derive("https://id.example.test/oidc", "external-user-42")
            .unwrap();
        assert_eq!(first, second);
        assert!(first.principal_id.starts_with("u1_"));
        assert_eq!(first.principal_id.len(), 46);
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("ID.Example"));
        assert!(!serialized.contains("external-user-42"));
        assert!(!format!("{:?}", deriver()).contains("070707"));
    }

    #[test]
    fn issuer_and_length_boundaries_separate_identities() {
        let deriver = deriver();
        assert_ne!(
            deriver.derive("https://issuer-a.test", "subject").unwrap(),
            deriver.derive("https://issuer-b.test", "subject").unwrap()
        );
        assert_ne!(
            deriver.derive("https://a.test/b", "c").unwrap(),
            deriver.derive("https://a.test", "bc").unwrap()
        );
    }

    #[test]
    fn malformed_issuer_and_subject_are_rejected() {
        let cases = [
            ("", "subject"),
            ("http://issuer.test", "subject"),
            ("https://user@issuer.test", "subject"),
            ("https://issuer.test?tenant=x", "subject"),
            ("https://issuer.test#fragment", "subject"),
            ("https://issuer.test", ""),
            ("https://issuer.test", "line\nbreak"),
        ];
        for (issuer, subject) in cases {
            assert!(matches!(
                deriver().derive(issuer, subject),
                Err(ApplicationError::InvalidArgument { .. })
            ));
        }
    }
}
