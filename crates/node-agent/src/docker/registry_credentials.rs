use super::{secrets::secret_transport_error, DockerRuntimeDriver};
use a3s_cloud_contracts::{CloudSecretReference, RegistryCredentialMaterial};
use a3s_runtime::contract::{RuntimeUnitSpec, SecretTarget};
use a3s_runtime::{RuntimeError, RuntimeResult};
use bollard::auth::DockerCredentials;

impl DockerRuntimeDriver {
    pub(super) async fn resolve_registry_credentials(
        &self,
        spec: &RuntimeUnitSpec,
        registry_address: &str,
    ) -> RuntimeResult<Option<DockerCredentials>> {
        let mut bindings = spec
            .secrets
            .iter()
            .filter(|secret| matches!(secret.target, SecretTarget::RegistryCredential));
        let Some(binding) = bindings.next() else {
            return Ok(None);
        };
        if bindings.next().is_some() {
            return Err(RuntimeError::InvalidRequest(
                "Docker Runtime specification has multiple registry credential Secrets".into(),
            ));
        }
        let reference = CloudSecretReference::parse(&binding.reference).map_err(|_| {
            RuntimeError::InvalidRequest(
                "Docker registry credential Secret reference is invalid".into(),
            )
        })?;
        let transport = self.secret_transport.read().await.clone().ok_or_else(|| {
            RuntimeError::ProviderUnavailable(
                "Docker Secret material transport is not bound".into(),
            )
        })?;
        let material = transport
            .resolve_secret(reference)
            .await
            .map_err(secret_transport_error)?;
        let credential = RegistryCredentialMaterial::parse(material.as_bytes())
            .map_err(|_| invalid_registry_credential())?;
        Ok(Some(DockerCredentials {
            username: Some(credential.username().to_owned()),
            password: Some(credential.password().to_owned()),
            serveraddress: Some(registry_address.to_owned()),
            ..DockerCredentials::default()
        }))
    }
}

fn invalid_registry_credential() -> RuntimeError {
    RuntimeError::InvalidRequest("Docker registry credential Secret material is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_is_closed_and_redacted() {
        let credential = RegistryCredentialMaterial::parse(
            br#"{"schema":"a3s.cloud.registry-credential.v1","username":"registry-user","password":"registry-password"}"#,
        )
        .expect("registry credential");
        assert_eq!(format!("{credential:?}"), "<redacted-registry-credential>");
        let error = RegistryCredentialMaterial::parse(
            br#"{"schema":"a3s.cloud.registry-credential.v1","username":"registry-user","password":"never-leak-this","extra":true}"#,
        )
        .expect_err("unknown credential field");
        assert!(!format!("{error:?}").contains("never-leak-this"));
        assert!(RegistryCredentialMaterial::parse(
            br#"{"schema":"a3s.cloud.registry-credential.v0","username":"registry-user","password":"registry-password"}"#,
        )
        .is_err());
    }
}
