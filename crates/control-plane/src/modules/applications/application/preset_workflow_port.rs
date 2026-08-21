use crate::modules::applications::domain::{
    ApplicationExperience, ApplicationWorkflowRevisionEvidence,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApplicationId, AssetId, AssetReleaseId, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const APPLICATION_PRESET_WORKFLOW_IDENTITY: &[u8] = b"cloud.application.preset-workflow.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationPresetModelRevision {
    pub model_id: Uuid,
    pub revision: String,
    pub digest: Sha256Digest,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationPresetAgentRelease {
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub digest: Sha256Digest,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ApplicationPresetTarget {
    ModelRevision(ApplicationPresetModelRevision),
    AgentRelease(ApplicationPresetAgentRelease),
}

impl ApplicationPresetTarget {
    fn validate(&self) -> Result<(), String> {
        let (resource_is_nil, revision, digest, capability) = match self {
            Self::ModelRevision(target) => (
                target.model_id.is_nil(),
                target.revision.clone(),
                &target.digest,
                target.capability.as_str(),
            ),
            Self::AgentRelease(target) => (
                target.asset_id.as_uuid().is_nil() || target.asset_release_id.as_uuid().is_nil(),
                target.asset_release_id.to_string(),
                &target.digest,
                target.capability.as_str(),
            ),
        };
        if resource_is_nil
            || revision.is_empty()
            || revision.len() > 128
            || !revision.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
            })
            || capability.is_empty()
            || capability.len() > 128
            || !capability.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.' | b'/')
            })
            || Sha256Digest::parse(digest.as_str())? != *digest
        {
            return Err("Application preset target is invalid".into());
        }
        Ok(())
    }
}

/// Applications-owned request for one generated wrapper Workflow.
///
/// The Application identity and release number are reserved before the
/// release is committed. They form a stable cross-context publication
/// identity; the target contains only an exact immutable capability reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationPresetWorkflowRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_number: u64,
    pub experience: ApplicationExperience,
    pub target: ApplicationPresetTarget,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl ApplicationPresetWorkflowRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_number == 0
            || self.actor_principal_id.as_uuid().is_nil()
            || self.request_id.is_nil()
            || self.idempotency_key.is_empty()
            || self.idempotency_key.len() > 255
            || self.idempotency_key.contains(['\0', '\r', '\n'])
        {
            return Err("Application preset Workflow request identity is invalid".into());
        }
        match (self.experience, &self.target) {
            (
                ApplicationExperience::Chatbot | ApplicationExperience::TextGenerator,
                ApplicationPresetTarget::ModelRevision(_),
            )
            | (
                ApplicationExperience::ClassicAgent | ApplicationExperience::NewAgent,
                ApplicationPresetTarget::AgentRelease(_),
            ) => {}
            (ApplicationExperience::Chatflow | ApplicationExperience::Workflow, _) => {
                return Err(
                    "Chatflow and Workflow require an exact user-authored Workflow revision".into(),
                )
            }
            _ => {
                return Err(
                    "Application preset experience and exact target type do not match".into(),
                )
            }
        }
        self.target.validate()
    }

    pub fn workflow_definition_id(&self) -> WorkflowDefinitionId {
        let mut identity = Vec::with_capacity(APPLICATION_PRESET_WORKFLOW_IDENTITY.len() + 49);
        identity.extend_from_slice(APPLICATION_PRESET_WORKFLOW_IDENTITY);
        identity.push(0);
        identity.extend_from_slice(self.project_id.as_uuid().as_bytes());
        identity.extend_from_slice(self.application_id.as_uuid().as_bytes());
        identity.extend_from_slice(&self.application_release_number.to_be_bytes());
        WorkflowDefinitionId::from_uuid(Uuid::new_v5(&self.organization_id.as_uuid(), &identity))
    }

    pub fn workflow_revision_id(&self) -> WorkflowRevisionId {
        WorkflowRevisionId::from_uuid(Uuid::new_v5(
            &self.workflow_definition_id().as_uuid(),
            b"initial-revision",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationPresetWorkflowResult {
    pub evidence: ApplicationWorkflowRevisionEvidence,
    pub replayed: bool,
}

#[async_trait]
pub trait IApplicationPresetWorkflowPort: Send + Sync {
    async fn compile_and_publish(
        &self,
        request: &ApplicationPresetWorkflowRequest,
    ) -> ApplicationResult<ApplicationPresetWorkflowResult>;
}
