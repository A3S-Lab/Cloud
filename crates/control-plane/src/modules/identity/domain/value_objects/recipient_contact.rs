use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_EMAIL_ADDRESS_BYTES: usize = 254;
const MAX_EMAIL_LOCAL_PART_BYTES: usize = 64;
const MAX_SIGNING_KEY_ID_BYTES: usize = 64;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecipientEmailAddress(String);

impl RecipientEmailAddress {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, String> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > MAX_EMAIL_ADDRESS_BYTES
            || !value.is_ascii()
            || value.trim() != value
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err("recipient email address must be a bounded canonical ASCII mailbox".into());
        }
        let mut parts = value.split('@');
        let local = parts.next().unwrap_or_default();
        let domain = parts.next().unwrap_or_default();
        if parts.next().is_some()
            || local.is_empty()
            || local.len() > MAX_EMAIL_LOCAL_PART_BYTES
            || domain.is_empty()
            || local.starts_with('.')
            || local.ends_with('.')
            || local.contains("..")
            || !local.bytes().all(valid_local_byte)
            || !valid_domain(domain)
        {
            return Err("recipient email address must be a bounded canonical ASCII mailbox".into());
        }
        Ok(Self(format!(
            "{}@{}",
            local.to_ascii_lowercase(),
            domain.to_ascii_lowercase()
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn digest(&self) -> Sha256Digest {
        Sha256Digest::from_bytes(self.0.as_bytes())
    }

    pub fn redacted_hint(&self) -> String {
        let domain = self
            .0
            .split_once('@')
            .map(|(_, domain)| domain)
            .unwrap_or("invalid");
        format!("***@{domain}")
    }
}

impl fmt::Debug for RecipientEmailAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecipientEmailAddress([REDACTED])")
    }
}

fn valid_local_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'.' | b'!'
                | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'/'
                | b'='
                | b'?'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
                | b'~'
        )
}

fn valid_domain(value: &str) -> bool {
    value.len() <= 253
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecipientContactSigningKeyId(String);

impl RecipientContactSigningKeyId {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_SIGNING_KEY_ID_BYTES
            || !value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase())
            || !value
                .bytes()
                .last()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
        {
            return Err(
                "recipient contact signing key ID must use bounded lowercase letters, digits, dots, hyphens, or underscores"
                    .into(),
            );
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
    fn email_is_canonical_bounded_and_redacted_in_debug_output() {
        let address =
            RecipientEmailAddress::parse("Operator+Alerts@Example.COM").expect("canonical address");
        assert_eq!(address.as_str(), "operator+alerts@example.com");
        assert_eq!(address.redacted_hint(), "***@example.com");
        assert!(!format!("{address:?}").contains("operator"));
        for invalid in [
            "",
            "operator",
            "operator@@example.com",
            ".operator@example.com",
            "operator..alerts@example.com",
            "operator@-example.com",
            "operator@example..com",
            " operator@example.com",
            "operator@例子.测试",
        ] {
            assert!(
                RecipientEmailAddress::parse(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn signing_key_identity_is_closed_and_canonical() {
        assert!(RecipientContactSigningKeyId::parse("recipient-contact-v1").is_ok());
        assert!(RecipientContactSigningKeyId::parse("RecipientContactV1").is_err());
        assert!(RecipientContactSigningKeyId::parse("-recipient-contact").is_err());
    }
}
