use serde::{Deserialize, Serialize};
use url::Url;

const MAX_PROVIDER_KEY_BYTES: usize = 63;
const MAX_EXTERNAL_SUBJECT_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OidcProviderKey(String);

impl OidcProviderKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_PROVIDER_KEY_BYTES
            && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            });
        if !valid {
            return Err(
                "OIDC provider key must use bounded lowercase letters, digits, hyphens, or underscores"
                    .into(),
            );
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OidcIssuer(String);

impl OidcIssuer {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 2048
            || !value.starts_with("https://")
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err("OIDC issuer must be a bounded canonical HTTPS URL".into());
        }
        let url =
            Url::parse(&value).map_err(|_| "OIDC issuer must be a bounded canonical HTTPS URL")?;
        let serialized = url.as_str();
        let exact_or_root_without_slash = serialized == value
            || (url.path() == "/"
                && serialized
                    .strip_suffix('/')
                    .is_some_and(|without_slash| without_slash == value));
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !exact_or_root_without_slash
        {
            return Err("OIDC issuer must be a bounded canonical HTTPS URL".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExternalIdentitySubject(String);

impl ExternalIdentitySubject {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_EXTERNAL_SUBJECT_BYTES
            || value.contains(['\0', '\r', '\n'])
        {
            return Err("external identity subject must contain 1 to 255 safe bytes".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_identity_uses_exact_bounded_values() {
        assert_eq!(
            OidcProviderKey::parse("workforce_oidc")
                .expect("provider key")
                .as_str(),
            "workforce_oidc"
        );
        assert!(OidcProviderKey::parse("Workforce").is_err());
        assert!(OidcProviderKey::parse("-workforce").is_err());

        assert_eq!(
            OidcIssuer::parse("https://identity.example.com/tenant")
                .expect("issuer")
                .as_str(),
            "https://identity.example.com/tenant"
        );
        assert!(OidcIssuer::parse("http://identity.example.com").is_err());
        assert!(OidcIssuer::parse("https://identity.example.com/").is_ok());
        assert!(OidcIssuer::parse("https://identity.example.com/tenant/").is_ok());
        assert!(OidcIssuer::parse("https://identity.example.com?tenant=one").is_err());
        assert!(OidcIssuer::parse(" https://identity.example.com").is_err());
        assert!(OidcIssuer::parse("HTTPS://identity.example.com").is_err());
        assert!(OidcIssuer::parse("https://IDENTITY.example.com").is_err());
        assert!(OidcIssuer::parse("https://identity.example.com/a/../tenant").is_err());

        assert!(ExternalIdentitySubject::parse("00u-subject-42").is_ok());
        assert!(ExternalIdentitySubject::parse("").is_err());
        assert!(ExternalIdentitySubject::parse("unsafe\nsubject").is_err());
    }
}
