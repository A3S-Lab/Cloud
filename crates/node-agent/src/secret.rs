use crate::NodeControlClientError;
use a3s_cloud_contracts::CloudSecretReference;
use async_trait::async_trait;
use zeroize::Zeroize;

#[async_trait]
pub trait NodeSecretTransport: Send + Sync {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError>;
}

#[derive(PartialEq, Eq)]
pub struct SecretMaterial(Vec<u8>);

impl SecretMaterial {
    pub fn new(value: impl Into<Vec<u8>>) -> Result<Self, String> {
        let mut value = value.into();
        if value.is_empty() || value.len() > 1024 * 1024 {
            value.zeroize();
            return Err("Secret material must contain between 1 byte and 1 MiB".into());
        }
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecretMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<redacted-secret-material>")
    }
}

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_debug_output_is_redacted() {
        let material = SecretMaterial::new(b"never-log-this".to_vec()).expect("Secret material");
        assert_eq!(format!("{material:?}"), "<redacted-secret-material>");
    }
}
