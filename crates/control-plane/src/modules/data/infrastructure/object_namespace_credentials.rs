use crate::modules::data::application::{
    ObjectNamespaceCredentialAdmission, ObjectNamespaceCredentialMaterializer,
};
use crate::modules::secrets::application::{
    exact_secret_materializer, exact_secret_version_access,
};
use crate::modules::secrets::domain::{ISecretEncryptionService, ISecretRepository};
use std::sync::Arc;

/// Data's sole composition adapter for the Secrets-owned exact-version
/// boundary. Data Application receives only the published interfaces; Secret
/// repositories, encryption, and owner-service construction remain here.
impl ObjectNamespaceCredentialAdmission {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self::from_secret_version_access(exact_secret_version_access(secrets))
    }
}

impl ObjectNamespaceCredentialMaterializer {
    pub fn new(
        secrets: Arc<dyn ISecretRepository>,
        encryption: Arc<dyn ISecretEncryptionService>,
    ) -> Self {
        Self::from_secret_materializer(exact_secret_materializer(secrets, encryption))
    }
}
