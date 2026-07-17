use crate::state_file::{self, SecureStateError, StateLock};
use a3s_cloud_contracts::{
    NodeCertificateRotationRequest, NodeCertificateRotationResponse, NodeEnrollmentRequest,
    NodeEnrollmentResponse,
};
use a3s_runtime::contract::RuntimeCapabilities;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

const IDENTITY_FILE: &str = "identity.json";
const IDENTITY_LOCK_FILE: &str = "identity.lock";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingNodeIdentity {
    pub agent_instance_id: Uuid,
    pub node_name: String,
    pub agent_version: String,
    pub runtime_capabilities: RuntimeCapabilities,
    private_key_pem: String,
    csr_pem: String,
}

impl std::fmt::Debug for PendingNodeIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingNodeIdentity")
            .field("agent_instance_id", &self.agent_instance_id)
            .field("node_name", &self.node_name)
            .field("agent_version", &self.agent_version)
            .field("runtime_capabilities", &self.runtime_capabilities)
            .field("private_key_pem", &"[REDACTED PEM]")
            .field("csr_pem", &"[REDACTED PEM]")
            .finish()
    }
}

impl PendingNodeIdentity {
    pub fn enrollment_request(&self, enrollment_token: String) -> NodeEnrollmentRequest {
        NodeEnrollmentRequest {
            schema: NodeEnrollmentRequest::SCHEMA.into(),
            enrollment_token,
            node_name: self.node_name.clone(),
            agent_instance_id: self.agent_instance_id,
            agent_version: self.agent_version.clone(),
            csr_pem: self.csr_pem.clone(),
            runtime_capabilities: self.runtime_capabilities.clone(),
        }
    }

    fn validate(&self) -> Result<(), IdentityStoreError> {
        if self.agent_instance_id.is_nil()
            || self.node_name.trim().is_empty()
            || self.node_name.len() > 255
            || self.node_name.contains(['\0', '\r', '\n'])
            || self.agent_version.trim().is_empty()
            || self.agent_version.len() > 255
            || self.agent_version.contains(['\0', '\r', '\n'])
        {
            return Err(IdentityStoreError::Invalid(
                "pending node identity metadata is invalid".into(),
            ));
        }
        self.runtime_capabilities
            .validate()
            .map_err(IdentityStoreError::Invalid)?;
        KeyPair::from_pem(&self.private_key_pem).map_err(|error| {
            IdentityStoreError::Invalid(format!("pending node private key is invalid: {error}"))
        })?;
        if !self
            .csr_pem
            .starts_with("-----BEGIN CERTIFICATE REQUEST-----")
            || !self
                .csr_pem
                .trim_end()
                .ends_with("-----END CERTIFICATE REQUEST-----")
        {
            return Err(IdentityStoreError::Invalid(
                "pending node CSR is invalid".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrolledNodeIdentity {
    pub agent_instance_id: Uuid,
    pub response: NodeEnrollmentResponse,
    private_key_pem: String,
    #[serde(default)]
    pending_rotation: Option<PendingCertificateRotation>,
}

impl std::fmt::Debug for EnrolledNodeIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnrolledNodeIdentity")
            .field("agent_instance_id", &self.agent_instance_id)
            .field("response", &self.response)
            .field("private_key_pem", &"[REDACTED PEM]")
            .field("pending_rotation", &self.pending_rotation)
            .finish()
    }
}

impl EnrolledNodeIdentity {
    pub fn identity_pem(&self) -> String {
        format!(
            "{}\n{}",
            self.response.certificate.certificate_pem, self.private_key_pem
        )
    }

    pub fn pending_rotation_request(&self) -> Option<NodeCertificateRotationRequest> {
        self.pending_rotation
            .as_ref()
            .map(|pending| pending.request(self.response.node_id))
    }

    pub fn has_pending_rotation(&self) -> bool {
        self.pending_rotation.is_some()
    }

    fn validate(&self) -> Result<(), IdentityStoreError> {
        if self.agent_instance_id.is_nil() {
            return Err(IdentityStoreError::Invalid(
                "enrolled agent instance ID is nil".into(),
            ));
        }
        self.response
            .validate()
            .map_err(IdentityStoreError::Invalid)?;
        KeyPair::from_pem(&self.private_key_pem).map_err(|error| {
            IdentityStoreError::Invalid(format!("enrolled node private key is invalid: {error}"))
        })?;
        if let Some(pending) = &self.pending_rotation {
            pending.validate(self.response.certificate.certificate_id)?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingCertificateRotation {
    current_certificate_id: Uuid,
    private_key_pem: String,
    csr_pem: String,
}

impl std::fmt::Debug for PendingCertificateRotation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingCertificateRotation")
            .field("current_certificate_id", &self.current_certificate_id)
            .field("private_key_pem", &"[REDACTED PEM]")
            .field("csr_pem", &"[REDACTED PEM]")
            .finish()
    }
}

impl PendingCertificateRotation {
    fn request(&self, node_id: Uuid) -> NodeCertificateRotationRequest {
        NodeCertificateRotationRequest {
            schema: NodeCertificateRotationRequest::SCHEMA.into(),
            node_id,
            current_certificate_id: self.current_certificate_id,
            csr_pem: self.csr_pem.clone(),
        }
    }

    fn validate(&self, active_certificate_id: Uuid) -> Result<(), IdentityStoreError> {
        if self.current_certificate_id != active_certificate_id {
            return Err(IdentityStoreError::Invalid(
                "pending rotation does not belong to the active node certificate".into(),
            ));
        }
        KeyPair::from_pem(&self.private_key_pem).map_err(|error| {
            IdentityStoreError::Invalid(format!("pending rotation private key is invalid: {error}"))
        })?;
        self.request(self.current_certificate_id)
            .validate()
            .map_err(IdentityStoreError::Invalid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum NodeIdentityState {
    Pending(PendingNodeIdentity),
    Enrolled(EnrolledNodeIdentity),
}

impl NodeIdentityState {
    fn validate(&self) -> Result<(), IdentityStoreError> {
        match self {
            Self::Pending(identity) => identity.validate(),
            Self::Enrolled(identity) => identity.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRecord {
    schema: String,
    identity: NodeIdentityState,
}

impl IdentityRecord {
    const SCHEMA: &'static str = "a3s.cloud.node-identity.v1";

    fn new(identity: NodeIdentityState) -> Result<Self, IdentityStoreError> {
        identity.validate()?;
        Ok(Self {
            schema: Self::SCHEMA.into(),
            identity,
        })
    }

    fn validate(&self) -> Result<(), IdentityStoreError> {
        if self.schema != Self::SCHEMA {
            return Err(IdentityStoreError::Invalid(format!(
                "unsupported node identity schema {:?}",
                self.schema
            )));
        }
        self.identity.validate()
    }
}

#[derive(Debug, Clone)]
pub struct FileNodeIdentityStore {
    root: PathBuf,
}

impl FileNodeIdentityStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn load(&self) -> Result<Option<NodeIdentityState>, IdentityStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.load_sync())
            .await
            .map_err(task_error)?
    }

    pub async fn prepare(
        &self,
        node_name: String,
        agent_version: String,
        runtime_capabilities: RuntimeCapabilities,
    ) -> Result<NodeIdentityState, IdentityStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            store.prepare_sync(node_name, agent_version, runtime_capabilities)
        })
        .await
        .map_err(task_error)?
    }

    pub async fn complete(
        &self,
        response: NodeEnrollmentResponse,
    ) -> Result<EnrolledNodeIdentity, IdentityStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.complete_sync(response))
            .await
            .map_err(task_error)?
    }

    pub async fn prepare_rotation(&self) -> Result<EnrolledNodeIdentity, IdentityStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.prepare_rotation_sync())
            .await
            .map_err(task_error)?
    }

    pub async fn complete_rotation(
        &self,
        response: NodeCertificateRotationResponse,
    ) -> Result<EnrolledNodeIdentity, IdentityStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.complete_rotation_sync(response))
            .await
            .map_err(task_error)?
    }

    fn load_sync(&self) -> Result<Option<NodeIdentityState>, IdentityStoreError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(IDENTITY_LOCK_FILE))?;
        self.read_record()
            .map(|record| record.map(|value| value.identity))
    }

    fn prepare_sync(
        &self,
        node_name: String,
        agent_version: String,
        runtime_capabilities: RuntimeCapabilities,
    ) -> Result<NodeIdentityState, IdentityStoreError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(IDENTITY_LOCK_FILE))?;
        if let Some(record) = self.read_record()? {
            return Ok(record.identity);
        }
        let private_key = KeyPair::generate().map_err(|error| {
            IdentityStoreError::Invalid(format!("could not generate node private key: {error}"))
        })?;
        let mut params = CertificateParams::default();
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, format!("a3s-node:{node_name}"));
        params.distinguished_name = distinguished_name;
        let csr_pem = params
            .serialize_request(&private_key)
            .and_then(|request| request.pem())
            .map_err(|error| {
                IdentityStoreError::Invalid(format!("could not create node CSR: {error}"))
            })?;
        let pending = PendingNodeIdentity {
            agent_instance_id: Uuid::now_v7(),
            node_name,
            agent_version,
            runtime_capabilities,
            private_key_pem: private_key.serialize_pem(),
            csr_pem,
        };
        let record = IdentityRecord::new(NodeIdentityState::Pending(pending))?;
        self.write_record(&record)?;
        Ok(record.identity)
    }

    fn complete_sync(
        &self,
        response: NodeEnrollmentResponse,
    ) -> Result<EnrolledNodeIdentity, IdentityStoreError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(IDENTITY_LOCK_FILE))?;
        let current = self.read_record()?.ok_or_else(|| {
            IdentityStoreError::Conflict("node enrollment was not prepared".into())
        })?;
        match current.identity {
            NodeIdentityState::Enrolled(existing) if existing.response == response => Ok(existing),
            NodeIdentityState::Enrolled(_) => Err(IdentityStoreError::Conflict(
                "node identity is already enrolled with different material".into(),
            )),
            NodeIdentityState::Pending(pending) => {
                if response.node_id.is_nil() {
                    return Err(IdentityStoreError::Invalid(
                        "enrollment response has a nil node ID".into(),
                    ));
                }
                let enrolled = EnrolledNodeIdentity {
                    agent_instance_id: pending.agent_instance_id,
                    response,
                    private_key_pem: pending.private_key_pem,
                    pending_rotation: None,
                };
                let record = IdentityRecord::new(NodeIdentityState::Enrolled(enrolled.clone()))?;
                self.write_record(&record)?;
                Ok(enrolled)
            }
        }
    }

    fn prepare_rotation_sync(&self) -> Result<EnrolledNodeIdentity, IdentityStoreError> {
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(IDENTITY_LOCK_FILE))?;
        let current = self
            .read_record()?
            .ok_or_else(|| IdentityStoreError::Conflict("node is not enrolled".into()))?;
        let mut enrolled = match current.identity {
            NodeIdentityState::Pending(_) => {
                return Err(IdentityStoreError::Conflict(
                    "node enrollment is not complete".into(),
                ))
            }
            NodeIdentityState::Enrolled(enrolled) => enrolled,
        };
        if enrolled.pending_rotation.is_some() {
            return Ok(enrolled);
        }
        let private_key = KeyPair::generate().map_err(|error| {
            IdentityStoreError::Invalid(format!(
                "could not generate replacement node private key: {error}"
            ))
        })?;
        let mut params = CertificateParams::default();
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(
            DnType::CommonName,
            format!("a3s-node:{}:rotation", enrolled.response.node_id),
        );
        params.distinguished_name = distinguished_name;
        let csr_pem = params
            .serialize_request(&private_key)
            .and_then(|request| request.pem())
            .map_err(|error| {
                IdentityStoreError::Invalid(format!(
                    "could not create replacement node CSR: {error}"
                ))
            })?;
        enrolled.pending_rotation = Some(PendingCertificateRotation {
            current_certificate_id: enrolled.response.certificate.certificate_id,
            private_key_pem: private_key.serialize_pem(),
            csr_pem,
        });
        let record = IdentityRecord::new(NodeIdentityState::Enrolled(enrolled.clone()))?;
        self.write_record(&record)?;
        Ok(enrolled)
    }

    fn complete_rotation_sync(
        &self,
        response: NodeCertificateRotationResponse,
    ) -> Result<EnrolledNodeIdentity, IdentityStoreError> {
        response.validate().map_err(IdentityStoreError::Invalid)?;
        state_file::ensure_directory(&self.root)?;
        let _lock = StateLock::exclusive(&self.root.join(IDENTITY_LOCK_FILE))?;
        let current = self
            .read_record()?
            .ok_or_else(|| IdentityStoreError::Conflict("node is not enrolled".into()))?;
        let mut enrolled = match current.identity {
            NodeIdentityState::Pending(_) => {
                return Err(IdentityStoreError::Conflict(
                    "node enrollment is not complete".into(),
                ))
            }
            NodeIdentityState::Enrolled(enrolled) => enrolled,
        };
        if response.node_id != enrolled.response.node_id {
            return Err(IdentityStoreError::Conflict(
                "certificate rotation changed the node identity".into(),
            ));
        }
        let Some(pending) = enrolled.pending_rotation.take() else {
            if enrolled.response.certificate == response.certificate {
                return Ok(enrolled);
            }
            return Err(IdentityStoreError::Conflict(
                "certificate rotation was not prepared".into(),
            ));
        };
        if response.previous_certificate_id != pending.current_certificate_id
            || enrolled.response.certificate.certificate_id != pending.current_certificate_id
        {
            return Err(IdentityStoreError::Conflict(
                "certificate rotation response does not match the prepared identity".into(),
            ));
        }
        enrolled.response.certificate = response.certificate;
        enrolled.private_key_pem = pending.private_key_pem;
        let record = IdentityRecord::new(NodeIdentityState::Enrolled(enrolled.clone()))?;
        self.write_record(&record)?;
        Ok(enrolled)
    }

    fn read_record(&self) -> Result<Option<IdentityRecord>, IdentityStoreError> {
        let path = self.root.join(IDENTITY_FILE);
        let record: Option<IdentityRecord> = state_file::read_json(&path, "node identity record")?;
        if let Some(record) = &record {
            record.validate()?;
        }
        Ok(record)
    }

    fn write_record(&self, record: &IdentityRecord) -> Result<(), IdentityStoreError> {
        state_file::atomic_write(&self.root.join(IDENTITY_FILE), record).map_err(Into::into)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityStoreError {
    #[error("invalid node identity: {0}")]
    Invalid(String),
    #[error("node identity conflict: {0}")]
    Conflict(String),
    #[error("node identity storage failed: {0}")]
    Storage(String),
}

impl From<SecureStateError> for IdentityStoreError {
    fn from(error: SecureStateError) -> Self {
        match error {
            SecureStateError::Invalid(message) => Self::Invalid(message),
            SecureStateError::Storage(message) => Self::Storage(message),
        }
    }
}

fn task_error(error: tokio::task::JoinError) -> IdentityStoreError {
    IdentityStoreError::Storage(format!("node identity task failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::NodeCertificate;
    use a3s_runtime::contract::{
        IsolationLevel, NetworkMode, ResourceControl, RuntimeFeature, RuntimeUnitClass,
    };
    use chrono::{Duration, Utc};

    fn capabilities() -> RuntimeCapabilities {
        RuntimeCapabilities {
            schema: RuntimeCapabilities::SCHEMA.into(),
            provider_id: a3s_runtime::ProviderId::parse("docker")
                .expect("test Docker provider ID must be valid"),
            provider_build: "docker-test".into(),
            unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
            artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
            isolation_levels: vec![IsolationLevel::Container],
            network_modes: vec![NetworkMode::None, NetworkMode::Service],
            mount_kinds: Vec::new(),
            health_check_kinds: Vec::new(),
            resource_controls: vec![ResourceControl::Cpu, ResourceControl::Memory],
            features: vec![RuntimeFeature::DurableIdentity],
        }
    }

    fn response(node_id: Uuid) -> NodeEnrollmentResponse {
        let issued_at = Utc::now();
        NodeEnrollmentResponse {
            schema: NodeEnrollmentResponse::SCHEMA.into(),
            node_id,
            certificate: NodeCertificate {
                certificate_id: Uuid::now_v7(),
                serial_number: "serial-1".into(),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at,
                expires_at: issued_at + Duration::hours(1),
            },
            heartbeat_interval_ms: 5_000,
            command_long_poll_ms: 25_000,
            certificate_rotation_window_ms: 15 * 60 * 1_000,
        }
    }

    #[tokio::test]
    async fn pending_identity_is_durable_and_completion_is_exactly_replayable() {
        let directory = tempfile::tempdir().expect("identity directory");
        let store = FileNodeIdentityStore::new(directory.path());
        let first = store
            .prepare("worker-1".into(), "0.1.0".into(), capabilities())
            .await
            .expect("prepare identity");
        let replay = store
            .prepare("changed-name".into(), "9.9.9".into(), capabilities())
            .await
            .expect("replay identity");
        assert_eq!(first, replay);
        let pending = match first {
            NodeIdentityState::Pending(value) => value,
            NodeIdentityState::Enrolled(_) => panic!("expected pending identity"),
        };
        let request = pending.enrollment_request(format!("a3sn_{}", "a".repeat(64)));
        request.validate().expect("valid enrollment request");
        let enrolled_response = response(Uuid::now_v7());
        let enrolled = store
            .complete(enrolled_response.clone())
            .await
            .expect("complete enrollment");
        assert_eq!(enrolled.response, enrolled_response);
        assert_eq!(
            store
                .complete(enrolled_response)
                .await
                .expect("completion replay"),
            enrolled
        );
        assert!(matches!(
            store.load().await.expect("load identity"),
            Some(NodeIdentityState::Enrolled(_))
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory.path().join(IDENTITY_FILE))
                .expect("identity metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[tokio::test]
    async fn identity_store_rejects_conflicting_completion() {
        let directory = tempfile::tempdir().expect("identity directory");
        let store = FileNodeIdentityStore::new(directory.path());
        store
            .prepare("worker-1".into(), "0.1.0".into(), capabilities())
            .await
            .expect("prepare identity");
        store
            .complete(response(Uuid::now_v7()))
            .await
            .expect("complete identity");
        assert!(store.complete(response(Uuid::now_v7())).await.is_err());
    }

    #[tokio::test]
    async fn certificate_rotation_is_prepared_durably_and_completed_atomically() {
        let directory = tempfile::tempdir().expect("identity directory");
        let store = FileNodeIdentityStore::new(directory.path());
        let node_id = Uuid::now_v7();
        store
            .prepare("worker-1".into(), "0.1.0".into(), capabilities())
            .await
            .expect("prepare enrollment");
        let original = store
            .complete(response(node_id))
            .await
            .expect("complete enrollment");
        let prepared = store.prepare_rotation().await.expect("prepare rotation");
        let request = prepared
            .pending_rotation_request()
            .expect("pending rotation request");
        request.validate().expect("valid rotation request");
        assert_eq!(request.node_id, node_id);
        assert_eq!(
            request.current_certificate_id,
            original.response.certificate.certificate_id
        );

        let reopened = FileNodeIdentityStore::new(directory.path());
        let replay = reopened
            .prepare_rotation()
            .await
            .expect("replay prepared rotation");
        assert_eq!(
            replay.pending_rotation_request(),
            Some(request.clone()),
            "a restart must replay the exact private key and CSR"
        );

        let issued_at = Utc::now();
        let replacement = NodeCertificate {
            certificate_id: Uuid::now_v7(),
            serial_number: "serial-2".into(),
            certificate_pem:
                "-----BEGIN CERTIFICATE-----\ndGVzdC1yb3RhdGVk\n-----END CERTIFICATE-----\n".into(),
            ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"
                .into(),
            issued_at,
            expires_at: issued_at + Duration::hours(1),
        };
        let response = NodeCertificateRotationResponse {
            schema: NodeCertificateRotationResponse::SCHEMA.into(),
            node_id,
            previous_certificate_id: request.current_certificate_id,
            certificate: replacement.clone(),
            replayed: false,
        };
        let completed = reopened
            .complete_rotation(response.clone())
            .await
            .expect("complete rotation");
        assert_eq!(completed.response.certificate, replacement);
        assert!(!completed.has_pending_rotation());
        assert_ne!(completed.identity_pem(), original.identity_pem());
        assert_eq!(
            reopened
                .complete_rotation(response)
                .await
                .expect("replay rotation completion"),
            completed
        );
    }
}
