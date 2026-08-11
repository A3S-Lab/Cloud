use super::Sha256Digest;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRequest {
    pub scope: String,
    pub key: String,
    pub request_digest: String,
}

impl IdempotencyRequest {
    pub fn new(
        scope: impl Into<String>,
        key: impl Into<String>,
        canonical_request: &[u8],
    ) -> Result<Self, String> {
        let request = Self {
            scope: scope.into(),
            key: key.into(),
            request_digest: format!("sha256:{:x}", Sha256::digest(canonical_request)),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.scope.is_empty()
            || self.scope.len() > 255
            || self.scope.contains(['\0', '\r', '\n'])
        {
            return Err("idempotency scope is invalid".into());
        }
        if self.key.is_empty() || self.key.len() > 255 || self.key.contains(['\0', '\r', '\n']) {
            return Err("idempotency key is invalid".into());
        }
        Sha256Digest::parse(&self.request_digest)
            .map(|_| ())
            .map_err(|_| "idempotency request digest is invalid".into())
    }

    pub fn storage_key(&self) -> (&str, &str) {
        (&self.scope, &self.key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotentWrite<T> {
    pub value: T,
    pub replayed: bool,
}

#[cfg(test)]
mod tests {
    use super::IdempotencyRequest;

    #[test]
    fn validation_rejects_mutated_keys_and_digests() {
        let mut request = IdempotencyRequest::new("scope", "key", b"body").expect("request");
        request.key.clear();
        assert!(request.validate().is_err());

        request.key = "key".into();
        request.request_digest = "sha256:not-a-digest".into();
        assert!(request.validate().is_err());
    }
}
