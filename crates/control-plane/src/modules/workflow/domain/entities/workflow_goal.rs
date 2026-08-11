use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowGoalId,
};
use crate::modules::workflow::domain::{PlanRevision, WorkflowGoalContract};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowGoal {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: WorkflowGoalId,
    pub contract: WorkflowGoalContract,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl WorkflowGoal {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: WorkflowGoalId,
        contract: WorkflowGoalContract,
        plan_revision: &PlanRevision,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            id,
            contract,
            plan_revision_id: plan_revision.id,
            plan_digest: plan_revision.digest.clone(),
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate(plan_revision)?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: WorkflowGoalId,
        contract_acl: &str,
        contract_digest: &str,
        input_digest: &str,
        plan_revision_id: PlanRevisionId,
        plan_digest: Sha256Digest,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
        plan_revision: &PlanRevision,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            id,
            contract: WorkflowGoalContract::restore(contract_acl, contract_digest, input_digest)?,
            plan_revision_id,
            plan_digest,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate(plan_revision)?;
        Ok(value)
    }

    pub fn validate(&self, plan_revision: &PlanRevision) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || plan_revision.organization_id != self.organization_id
            || plan_revision.project_id != self.project_id
            || plan_revision.workflow_goal_id != self.id
            || plan_revision.id != self.plan_revision_id
            || plan_revision.digest != self.plan_digest
            || plan_revision.created_at != self.created_at
            || plan_revision.created_by != self.created_by
            || plan_revision.plan.workflow_definition_id
                != self.contract.spec().workflow_definition_id
            || plan_revision.plan.workflow_revision_id != self.contract.spec().workflow_revision_id
            || plan_revision.plan.workflow_digest != self.contract.spec().workflow_digest
            || plan_revision.plan.ontology_id != self.contract.spec().ontology_id
            || plan_revision.plan.ontology_revision_id != self.contract.spec().ontology_revision_id
            || plan_revision.plan.ontology_digest != self.contract.spec().ontology_digest
            || plan_revision.plan.input_digest != *self.contract.input_digest()
            || plan_revision.plan.environment_id != self.contract.spec().environment_id
        {
            return Err("stored WorkflowGoal and PlanRevision do not match".into());
        }
        plan_revision.validate()
    }
}
