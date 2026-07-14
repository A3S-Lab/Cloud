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
        let scope = scope.into();
        let key = key.into();
        if scope.is_empty() || scope.len() > 255 || scope.contains(['\0', '\r', '\n']) {
            return Err("idempotency scope is invalid".into());
        }
        if key.is_empty() || key.len() > 255 || key.contains(['\0', '\r', '\n']) {
            return Err("idempotency key is invalid".into());
        }
        Ok(Self {
            scope,
            key,
            request_digest: format!("sha256:{:x}", Sha256::digest(canonical_request)),
        })
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
