use super::ApplicationError;
use crate::modules::shared_kernel::domain::Sha256Digest;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const OAUTH_FLOW_SECRET_BYTES: usize = 32;
pub const OAUTH_FLOW_SECRET_LENGTH: usize = 43;

pub fn generate_oauth_flow_secret(purpose: &str) -> Result<Zeroizing<String>, ApplicationError> {
    let mut random = Zeroizing::new([0_u8; OAUTH_FLOW_SECRET_BYTES]);
    getrandom::fill(&mut *random).map_err(|error| {
        ApplicationError::Internal(format!("could not generate {purpose}: {error}"))
    })?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(&random[..])))
}

pub fn validate_oauth_flow_secret(
    value: Zeroizing<String>,
    label: &str,
) -> Result<Zeroizing<String>, ApplicationError> {
    if value.len() != OAUTH_FLOW_SECRET_LENGTH
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ApplicationError::Invalid(format!("{label} is invalid")));
    }
    Ok(value)
}

pub fn oauth_flow_digest(value: &str) -> Sha256Digest {
    Sha256Digest::from_bytes(value.as_bytes())
}

pub fn pkce_s256_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_material_is_url_safe_bounded_and_digestible() {
        let left = generate_oauth_flow_secret("test OAuth state").expect("state");
        let right = generate_oauth_flow_secret("test OAuth state").expect("state");

        assert_eq!(left.len(), OAUTH_FLOW_SECRET_LENGTH);
        assert_ne!(left.as_str(), right.as_str());
        assert!(validate_oauth_flow_secret(left.clone(), "OAuth state").is_ok());
        assert!(oauth_flow_digest(&left).as_str().starts_with("sha256:"));
        assert_eq!(pkce_s256_challenge(&left).len(), OAUTH_FLOW_SECRET_LENGTH);
    }

    #[test]
    fn validation_rejects_wrong_lengths_and_non_url_safe_bytes() {
        assert!(validate_oauth_flow_secret(
            Zeroizing::new("a".repeat(OAUTH_FLOW_SECRET_LENGTH - 1)),
            "OAuth state"
        )
        .is_err());
        assert!(validate_oauth_flow_secret(
            Zeroizing::new(format!("{}+", "a".repeat(OAUTH_FLOW_SECRET_LENGTH - 1))),
            "OAuth state"
        )
        .is_err());
    }
}
