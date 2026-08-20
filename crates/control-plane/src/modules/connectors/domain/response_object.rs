use super::{
    ConnectorExecutionEvidence, ConnectorExecutionOutcome, ConnectorExecutionReceipt,
    ConnectorRevision, MAXIMUM_CONNECTOR_BODY_BYTES,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId, Sha256Digest,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CONNECTOR_RESPONSE_OBJECT_SCHEMA: &str = "cloud.connector.response-object.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorResponseObjectReference {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub connector_profile_id: ConnectorProfileId,
    pub connector_revision_id: ConnectorRevisionId,
    pub connector_attempt_id: Uuid,
    pub object_ref: String,
    pub digest: Sha256Digest,
    pub size_bytes: u64,
}

impl ConnectorResponseObjectReference {
    pub fn from_accepted(
        revision: &ConnectorRevision,
        receipt: &ConnectorExecutionReceipt,
    ) -> Result<Self, String> {
        if receipt.connector_revision_id() != revision.id {
            return Err("Connector response object changed its revision authority".into());
        }
        Self::new(
            revision.organization_id,
            revision.project_id,
            revision.environment_id,
            revision.profile_id,
            revision.id,
            receipt.attempt_id(),
            Sha256Digest::from_bytes(receipt.response_body()),
            receipt.response_body().len() as u64,
        )
    }

    pub fn from_evidence(evidence: &ConnectorExecutionEvidence) -> Result<Self, String> {
        evidence.validate()?;
        if evidence.outcome() != ConnectorExecutionOutcome::Accepted {
            return Err("Connector response objects require accepted evidence".into());
        }
        Self::new(
            evidence.organization_id(),
            evidence.project_id(),
            evidence.environment_id(),
            evidence.profile_id(),
            evidence.revision_id(),
            evidence.attempt_id(),
            evidence
                .response_digest()
                .cloned()
                .ok_or_else(|| "accepted Connector evidence lost its response digest".to_owned())?,
            evidence.response_body_bytes().ok_or_else(|| {
                "accepted Connector evidence lost its response byte count".to_owned()
            })?,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        connector_profile_id: ConnectorProfileId,
        connector_revision_id: ConnectorRevisionId,
        connector_attempt_id: Uuid,
        digest: Sha256Digest,
        size_bytes: u64,
    ) -> Result<Self, String> {
        let object_ref = derive_object_ref(connector_attempt_id, &digest)?;
        let reference = Self {
            schema: CONNECTOR_RESPONSE_OBJECT_SCHEMA.into(),
            organization_id,
            project_id,
            environment_id,
            connector_profile_id,
            connector_revision_id,
            connector_attempt_id,
            object_ref,
            digest,
            size_bytes,
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RESPONSE_OBJECT_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.connector_profile_id.as_uuid().is_nil()
            || self.connector_revision_id.as_uuid().is_nil()
            || self.connector_attempt_id.is_nil()
            || self.size_bytes > MAXIMUM_CONNECTOR_BODY_BYTES as u64
            || Sha256Digest::parse(self.digest.as_str()).ok().as_ref() != Some(&self.digest)
            || self.object_ref != derive_object_ref(self.connector_attempt_id, &self.digest)?
        {
            return Err("Connector response object reference is invalid".into());
        }
        Ok(())
    }

    pub fn validate_evidence(&self, evidence: &ConnectorExecutionEvidence) -> Result<(), String> {
        self.validate()?;
        if Self::from_evidence(evidence)? != *self {
            return Err("Connector response object changed its terminal evidence authority".into());
        }
        Ok(())
    }

    pub(crate) fn storage_key(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!(
            "organizations/{}/projects/{}/environments/{}/profiles/{}/revisions/{}/{}",
            self.organization_id,
            self.project_id,
            self.environment_id,
            self.connector_profile_id,
            self.connector_revision_id,
            self.object_ref,
        ))
    }
}

fn derive_object_ref(attempt_id: Uuid, digest: &Sha256Digest) -> Result<String, String> {
    if attempt_id.is_nil() {
        return Err("Connector response object attempt identity is invalid".into());
    }
    let hexadecimal = digest
        .as_str()
        .strip_prefix("sha256:")
        .ok_or_else(|| "Connector response object digest is invalid".to_owned())?;
    Sha256Digest::parse(format!("sha256:{hexadecimal}"))?;
    Ok(format!("attempts/{attempt_id}/sha256/{hexadecimal}/body"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorResponseObjectWrite {
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectorResponseObjectError {
    #[error("Connector response object request is invalid: {0}")]
    Invalid(String),
    #[error("Connector response object conflicts with existing content: {0}")]
    Conflict(String),
    #[error("Connector response object was not found")]
    NotFound,
    #[error("Connector response object failed integrity validation: {0}")]
    Integrity(String),
    #[error("Connector response object storage is unavailable: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait IConnectorResponseObjectStore: Send + Sync {
    async fn put(
        &self,
        reference: &ConnectorResponseObjectReference,
        body: Vec<u8>,
    ) -> Result<ConnectorResponseObjectWrite, ConnectorResponseObjectError>;

    async fn get(
        &self,
        reference: &ConnectorResponseObjectReference,
    ) -> Result<Vec<u8>, ConnectorResponseObjectError>;

    async fn verify(
        &self,
        reference: &ConnectorResponseObjectReference,
    ) -> Result<(), ConnectorResponseObjectError> {
        self.get(reference).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDefinition,
        ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
        ConnectorHttpStatusPolicy,
    };
    use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId};
    use chrono::Utc;

    fn accepted() -> (ConnectorRevision, ConnectorExecutionReceipt) {
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://response.example.test/execute".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 1_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            canonical_timestamp(Utc::now()),
        )
        .expect("revision");
        let receipt = ConnectorExecutionReceipt::accepted(
            revision.id,
            Uuid::now_v7(),
            canonical_timestamp(Utc::now()),
            200,
            Some("application/json".into()),
            br#"{"accepted":true}"#.to_vec(),
        )
        .expect("receipt");
        (revision, receipt)
    }

    #[test]
    fn reference_is_exact_attempt_scoped_and_content_addressed() {
        let (revision, receipt) = accepted();
        let reference = ConnectorResponseObjectReference::from_accepted(&revision, &receipt)
            .expect("response reference");
        reference.validate().expect("valid reference");
        assert!(reference
            .object_ref
            .starts_with(&format!("attempts/{}/sha256/", receipt.attempt_id())));
        assert!(reference.object_ref.ends_with("/body"));
        assert_eq!(
            reference.digest,
            Sha256Digest::from_bytes(receipt.response_body())
        );
        assert_eq!(reference.size_bytes, receipt.response_body().len() as u64);
    }

    #[test]
    fn reference_rejects_owner_digest_size_and_path_drift() {
        let (revision, receipt) = accepted();
        let reference = ConnectorResponseObjectReference::from_accepted(&revision, &receipt)
            .expect("response reference");

        let mut drifted = reference.clone();
        drifted.connector_revision_id = ConnectorRevisionId::new();
        // A changed owner remains structurally valid but cannot validate against
        // the terminal evidence that created the reference.
        assert!(drifted.validate().is_ok());

        drifted = reference.clone();
        drifted.object_ref.push_str("-changed");
        assert!(drifted.validate().is_err());

        drifted = reference;
        drifted.size_bytes = MAXIMUM_CONNECTOR_BODY_BYTES as u64 + 1;
        assert!(drifted.validate().is_err());
    }
}
