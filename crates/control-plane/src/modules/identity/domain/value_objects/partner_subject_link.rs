//! Partner directory subject-link admission (Kense SubjectLink and peers).
//!
//! Reuses `ExternalIdentityLink` storage. Partner links are administrator-managed
//! mappings from a partner directory subject to one human Cloud Principal.
//! They are not OIDC browser flows and must use a reserved provider key prefix.

use super::{ExternalIdentitySubject, OidcIssuer, OidcProviderKey};
use uuid::Uuid;

/// Reserved provider-key prefix for partner directory SubjectLink mappings.
pub const PARTNER_SUBJECT_PROVIDER_PREFIX: &str = "partner-";

/// Canonical provider key for the Kense directory extension plane.
pub const KENSE_DIRECTORY_PROVIDER_KEY: &str = "partner-kense-directory";

pub fn parse_partner_provider_key(value: impl Into<String>) -> Result<OidcProviderKey, String> {
    let key = OidcProviderKey::parse(value)?;
    if !key.as_str().starts_with(PARTNER_SUBJECT_PROVIDER_PREFIX) {
        return Err(format!(
            "partner subject provider key must start with `{PARTNER_SUBJECT_PROVIDER_PREFIX}`"
        ));
    }
    Ok(key)
}

pub fn parse_partner_directory_issuer(value: impl Into<String>) -> Result<OidcIssuer, String> {
    OidcIssuer::parse(value).map_err(|error| {
        format!("partner directory issuer must be a bounded canonical HTTPS URL: {error}")
    })
}

/// Partner directory subjects are stable opaque IDs; Kense uses UUID text.
pub fn parse_partner_directory_subject(value: impl Into<String>) -> Result<ExternalIdentitySubject, String> {
    let raw = value.into();
    let parsed = Uuid::parse_str(raw.trim()).map_err(|_| {
        "partner directory subject must be a canonical UUID string (no email keys)".to_owned()
    })?;
    ExternalIdentitySubject::parse(parsed.to_string())
}

pub fn is_partner_provider_key(key: &OidcProviderKey) -> bool {
    key.as_str().starts_with(PARTNER_SUBJECT_PROVIDER_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_kense_directory_provider_and_uuid_subject() {
        let key = parse_partner_provider_key(KENSE_DIRECTORY_PROVIDER_KEY).expect("key");
        assert!(is_partner_provider_key(&key));
        let issuer =
            parse_partner_directory_issuer("https://kense.example/directory").expect("issuer");
        assert_eq!(issuer.as_str(), "https://kense.example/directory");
        let subject = parse_partner_directory_subject("550e8400-e29b-41d4-a716-446655440000")
            .expect("subject");
        assert_eq!(subject.as_str(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn rejects_oidc_provider_keys_and_email_subjects() {
        assert!(parse_partner_provider_key("workforce").is_err());
        assert!(parse_partner_directory_subject("user@example.com").is_err());
    }
}
