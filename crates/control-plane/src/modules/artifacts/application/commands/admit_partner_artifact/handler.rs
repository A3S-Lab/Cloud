use super::AdmitPartnerArtifact;
use crate::modules::artifacts::application::commands::admit_partner_artifact::AdmitPartnerArtifactResult;
use crate::modules::artifacts::domain::entities::{
    PartnerArtifactAdmission, PartnerArtifactAdmissionId, PartnerArtifactKind,
};
use crate::modules::artifacts::domain::repositories::{
    AdmitPartnerArtifactWrite, IPartnerArtifactAdmissionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, Sha256Digest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct AdmitPartnerArtifactHandler {
    repository: Arc<dyn IPartnerArtifactAdmissionRepository>,
}

impl AdmitPartnerArtifactHandler {
    pub fn new(repository: Arc<dyn IPartnerArtifactAdmissionRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<AdmitPartnerArtifact> for AdmitPartnerArtifactHandler {
    fn execute(
        &self,
        command: AdmitPartnerArtifact,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AdmitPartnerArtifactResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let content_digest = match Sha256Digest::parse(command.content_digest) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let kind = match PartnerArtifactKind::parse(&command.kind) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let admission = match PartnerArtifactAdmission::create(
                PartnerArtifactAdmissionId::new(),
                command.organization_id,
                content_digest,
                kind,
                command.byte_size,
                command.partner_ref,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "contentDigest": admission.content_digest.as_str(),
                "kind": admission.kind.as_str(),
                "byteSize": admission.byte_size,
                "partnerRef": admission.partner_ref,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/partner-artifact-admissions",
                    command.organization_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let _ = command.request_id;
            match repository
                .admit(AdmitPartnerArtifactWrite {
                    admission,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(AdmitPartnerArtifactResult {
                    admission: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
