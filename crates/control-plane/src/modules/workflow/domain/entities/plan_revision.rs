use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, EnvironmentId, OntologyId,
    OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{
    CapabilityReference, WorkflowContractQuotas, WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind,
    WorkflowStepSpec,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const WORKFLOW_PLAN_SCHEMA: &str = "cloud.workflow.plan.v1";
pub const WORKFLOW_PLAN_COMPILER_REVISION: &str = "cloud.workflow.plan-compiler.v1";
pub const WORKFLOW_PLAN_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlanStep {
    pub id: String,
    pub kind: WorkflowStepKind,
    pub configuration_digest: Sha256Digest,
    pub input_schema_digest: Sha256Digest,
    pub output_schema_digest: Sha256Digest,
    pub policy_digest: Option<Sha256Digest>,
    pub capability: Option<CapabilityReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlan {
    pub schema: String,
    pub compiler_revision: String,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub workflow_revision_id: WorkflowRevisionId,
    pub workflow_digest: Sha256Digest,
    pub workflow_payload_set_digest: Sha256Digest,
    pub ontology_id: OntologyId,
    pub ontology_revision_id: OntologyRevisionId,
    pub ontology_digest: Sha256Digest,
    pub environment_id: Option<EnvironmentId>,
    pub input_digest: Sha256Digest,
    pub steps: Vec<WorkflowPlanStep>,
    pub edges: Vec<WorkflowEdgeSpec>,
}

impl WorkflowPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_PLAN_SCHEMA
            || self.compiler_revision != WORKFLOW_PLAN_COMPILER_REVISION
            || self.workflow_definition_id.as_uuid().is_nil()
            || self.workflow_revision_id.as_uuid().is_nil()
            || self.ontology_id.as_uuid().is_nil()
            || self.ontology_revision_id.as_uuid().is_nil()
            || self
                .environment_id
                .is_some_and(|environment_id| environment_id.as_uuid().is_nil())
        {
            return Err("Workflow plan authority bindings are invalid".into());
        }
        let mut ids = BTreeSet::new();
        let workflow = WorkflowSpec {
            name: "Compiled Workflow plan".into(),
            description: String::new(),
            steps: self
                .steps
                .iter()
                .map(|step| {
                    if !ids.insert(step.id.as_str()) {
                        return Err(format!(
                            "Workflow plan contains duplicate step ID {:?}",
                            step.id
                        ));
                    }
                    Ok(WorkflowStepSpec {
                        id: step.id.clone(),
                        label: step.id.clone(),
                        kind: step.kind,
                        configuration_digest: step.configuration_digest.clone(),
                        input_schema_digest: step.input_schema_digest.clone(),
                        output_schema_digest: step.output_schema_digest.clone(),
                        policy_digest: step.policy_digest.clone(),
                        capability: step.capability.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
            edges: self.edges.clone(),
        };
        let order = workflow.topological_order(WorkflowContractQuotas::default())?;
        if self.environment_id.is_none()
            && self
                .steps
                .iter()
                .any(|step| step.kind == WorkflowStepKind::Execution)
        {
            return Err("Workflow plans with Execution steps require one exact environment".into());
        }
        let stored_order = self
            .steps
            .iter()
            .map(|step| step.id.clone())
            .collect::<Vec<_>>();
        if order != stored_order {
            return Err("Workflow plan steps are not in deterministic topological order".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_goal_id: WorkflowGoalId,
    pub id: PlanRevisionId,
    pub plan: WorkflowPlan,
    pub canonical_plan: String,
    pub digest: Sha256Digest,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl PlanRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_goal_id: WorkflowGoalId,
        id: PlanRevisionId,
        plan: WorkflowPlan,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        plan.validate()?;
        let canonical = canonical_json_bounded(&plan, WORKFLOW_PLAN_MAX_BYTES, "Workflow plan")?;
        let canonical_plan = String::from_utf8(canonical.clone())
            .map_err(|_| "Workflow plan did not encode as UTF-8".to_owned())?;
        let digest = Sha256Digest::parse(sha256_digest(&canonical))?;
        let value = Self {
            organization_id,
            project_id,
            workflow_goal_id,
            id,
            plan,
            canonical_plan,
            digest,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_goal_id: WorkflowGoalId,
        id: PlanRevisionId,
        canonical_plan: &str,
        stored_digest: &str,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if canonical_plan.is_empty() || canonical_plan.len() > WORKFLOW_PLAN_MAX_BYTES {
            return Err("stored Workflow plan size is invalid".into());
        }
        let plan = serde_json::from_str(canonical_plan)
            .map_err(|error| format!("stored Workflow plan is invalid JSON: {error}"))?;
        let value = Self::create(
            organization_id,
            project_id,
            workflow_goal_id,
            id,
            plan,
            created_by,
            created_at,
        )?;
        if value.canonical_plan != canonical_plan || value.digest.as_str() != stored_digest {
            return Err("stored Workflow plan and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_goal_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
        {
            return Err("stored PlanRevision identity is invalid".into());
        }
        self.plan.validate()?;
        let canonical =
            canonical_json_bounded(&self.plan, WORKFLOW_PLAN_MAX_BYTES, "Workflow plan")?;
        if canonical.as_slice() != self.canonical_plan.as_bytes()
            || sha256_digest(&canonical) != self.digest.as_str()
        {
            return Err("stored PlanRevision canonical content is invalid".into());
        }
        Ok(())
    }
}
