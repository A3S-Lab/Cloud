use crate::modules::edge::domain::{DomainClaim, DomainClaimState};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, OrganizationId, ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainClaimChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub domain_claim_id: DomainClaimId,
    pub pattern: String,
    pub state: DomainClaimState,
    pub failure: Option<String>,
}

impl DomainClaimChanged {
    pub fn envelope(
        claim: &DomainClaim,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let event_key = match claim.state {
            DomainClaimState::Pending => "edge.domain-claim.created",
            DomainClaimState::Verified => "edge.domain-claim.verified",
            DomainClaimState::Rejected => "edge.domain-claim.rejected",
            DomainClaimState::Revoked => "edge.domain-claim.revoked",
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: claim.organization_id.as_uuid(),
            aggregate_id: claim.id.as_uuid(),
            aggregate_version: claim.aggregate_version,
            occurred_at: claim.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: claim.organization_id,
                project_id: claim.project_id,
                environment_id: claim.environment_id,
                domain_claim_id: claim.id,
                pattern: claim.pattern.as_str().into(),
                state: claim.state,
                failure: claim.failure.clone(),
            })?,
        })
    }
}
