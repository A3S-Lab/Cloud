use super::{
    WorkflowCompositeFrame, WorkflowCompositeFrameResolution, WorkflowCompositeRegionResultRequest,
    WorkflowCompositeRegions, WorkflowPlan, WorkflowRunRecord, WorkflowVariableContract,
    WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, OperationId, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const WORKFLOW_COMPOSITE_HOOK_SCHEMA: &str = "cloud.workflow.composite-hook.v1";
pub const WORKFLOW_COMPOSITE_RESUME_SCHEMA: &str = "cloud.workflow.composite-resume.v1";
pub const WORKFLOW_COMPOSITE_CHILD_REFERENCE_SCHEMA: &str =
    "cloud.workflow.composite-child-reference.v1";
const WORKFLOW_COMPOSITE_CHILD_ID_VERSION: &str = "cloud.workflow.composite-child.v1";

impl WorkflowCompositeFrame {
    pub fn child_workflow_run_id(&self) -> WorkflowRunId {
        let identity = format!(
            "{WORKFLOW_COMPOSITE_CHILD_ID_VERSION}:{}",
            self.frame_digest
        );
        WorkflowRunId::from_uuid(Uuid::new_v5(
            &self.workflow_run_id.as_uuid(),
            identity.as_bytes(),
        ))
    }

    pub fn child_workflow_goal_id(&self) -> WorkflowGoalId {
        WorkflowGoalId::from_uuid(Uuid::new_v5(
            &self.child_workflow_run_id().as_uuid(),
            b"goal",
        ))
    }

    pub fn child_plan_revision_id(&self) -> PlanRevisionId {
        PlanRevisionId::from_uuid(Uuid::new_v5(
            &self.child_workflow_run_id().as_uuid(),
            b"plan",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeHookMetadata {
    pub schema: String,
    pub frame: WorkflowCompositeFrame,
}

impl WorkflowCompositeHookMetadata {
    pub fn new(
        frame: WorkflowCompositeFrame,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKFLOW_COMPOSITE_HOOK_SCHEMA.into(),
            frame,
        };
        value.validate(plan, regions, variables)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
    ) -> Result<(), String> {
        if self.schema != WORKFLOW_COMPOSITE_HOOK_SCHEMA {
            return Err("Workflow composite hook schema is invalid".into());
        }
        self.frame.validate(plan, regions, variables)?;
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite hook metadata",
        )?;
        Ok(())
    }

    pub fn flow_hook_id(&self) -> String {
        format!(
            "workflow-composite:{}:{}",
            self.frame.region_step_id, self.frame.ordinal
        )
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-composite:{}:{}:{}:{}",
            self.frame.workflow_run_id,
            self.frame.region_step_id,
            self.frame.ordinal,
            self.frame.frame_digest
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeResumePayload {
    pub schema: String,
    pub workflow_run_id: WorkflowRunId,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub frame_digest: Sha256Digest,
    pub resolution: WorkflowCompositeFrameResolution,
    pub payload_digest: Sha256Digest,
}

impl WorkflowCompositeResumePayload {
    pub fn new(
        metadata: &WorkflowCompositeHookMetadata,
        resolution: WorkflowCompositeFrameResolution,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: WORKFLOW_COMPOSITE_RESUME_SCHEMA.into(),
            workflow_run_id: metadata.frame.workflow_run_id,
            flow_run_id: metadata.frame.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            frame_digest: metadata.frame.frame_digest.clone(),
            resolution,
            payload_digest: Sha256Digest::from_bytes(&[]),
        };
        value.payload_digest = value.compute_digest()?;
        value.validate(metadata, plan, regions, variables)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        metadata: &WorkflowCompositeHookMetadata,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
    ) -> Result<(), String> {
        metadata.validate(plan, regions, variables)?;
        if self.schema != WORKFLOW_COMPOSITE_RESUME_SCHEMA
            || self.workflow_run_id != metadata.frame.workflow_run_id
            || self.flow_run_id != metadata.frame.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
            || self.frame_digest != metadata.frame.frame_digest
            || self.resolution.frame() != &metadata.frame
        {
            return Err("Workflow composite resume authority drifted".into());
        }
        self.resolution.validate(
            &WorkflowCompositeRegionResultRequest {
                organization_id: metadata.frame.organization_id,
                project_id: metadata.frame.project_id,
                workflow_run_id: metadata.frame.workflow_run_id,
                plan_revision_id: metadata.frame.plan_revision_id,
                plan_digest: metadata.frame.plan_digest.clone(),
                region_step_id: metadata.frame.region_step_id.clone(),
            },
            plan,
            regions,
            variables,
            metadata.frame.mode,
        )?;
        if self.payload_digest != self.compute_digest()? {
            return Err("Workflow composite resume digest drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite resume payload",
        )?;
        Ok(())
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
            &WorkflowCompositeResumeDigestBody {
                schema: &self.schema,
                workflow_run_id: self.workflow_run_id,
                flow_run_id: &self.flow_run_id,
                flow_hook_id: &self.flow_hook_id,
                frame_digest: &self.frame_digest,
                resolution: &self.resolution,
            },
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite resume digest body",
        )?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeChildReferenceMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub parent_workflow_run_id: WorkflowRunId,
    pub parent_plan_revision_id: PlanRevisionId,
    pub parent_plan_digest: Sha256Digest,
    pub region_step_id: String,
    pub ordinal: u32,
    pub frame_digest: Sha256Digest,
    pub child_workflow_definition_id: WorkflowDefinitionId,
    pub child_workflow_revision_id: WorkflowRevisionId,
    pub child_workflow_digest: Sha256Digest,
    pub child_workflow_run_id: WorkflowRunId,
    pub child_workflow_goal_id: WorkflowGoalId,
    pub child_operation_id: OperationId,
    pub child_plan_revision_id: PlanRevisionId,
    pub child_plan_digest: Sha256Digest,
    pub child_input_digest: Sha256Digest,
}

impl WorkflowCompositeChildReferenceMetadata {
    pub fn new(
        hook: &WorkflowCompositeHookMetadata,
        child: &WorkflowRunRecord,
    ) -> Result<Self, String> {
        child.validate()?;
        let frame = &hook.frame;
        if child.run.organization_id != frame.organization_id
            || child.run.project_id != frame.project_id
            || child.run.execution_input.plan.workflow_definition_id
                != frame.child_workflow_definition_id
            || child.run.execution_input.plan.workflow_revision_id
                != frame.child_workflow_revision_id
            || child.run.execution_input.plan.workflow_digest != frame.child_workflow_digest
            || child.run.execution_input.goal_input != frame.child_input
            || child.run.execution_input.plan.input_digest != frame.child_input_digest
        {
            return Err("Workflow composite child run authority drifted".into());
        }
        let value = Self {
            schema: WORKFLOW_COMPOSITE_CHILD_REFERENCE_SCHEMA.into(),
            organization_id: frame.organization_id,
            project_id: frame.project_id,
            parent_workflow_run_id: frame.workflow_run_id,
            parent_plan_revision_id: frame.plan_revision_id,
            parent_plan_digest: frame.plan_digest.clone(),
            region_step_id: frame.region_step_id.clone(),
            ordinal: frame.ordinal,
            frame_digest: frame.frame_digest.clone(),
            child_workflow_definition_id: frame.child_workflow_definition_id,
            child_workflow_revision_id: frame.child_workflow_revision_id,
            child_workflow_digest: frame.child_workflow_digest.clone(),
            child_workflow_run_id: child.run.id,
            child_workflow_goal_id: child.run.workflow_goal_id,
            child_operation_id: child.run.operation_id,
            child_plan_revision_id: child.run.plan_revision_id,
            child_plan_digest: child.run.plan_digest.clone(),
            child_input_digest: frame.child_input_digest.clone(),
        };
        value.validate(hook)?;
        Ok(value)
    }

    pub fn validate(&self, hook: &WorkflowCompositeHookMetadata) -> Result<(), String> {
        let frame = &hook.frame;
        if self.schema != WORKFLOW_COMPOSITE_CHILD_REFERENCE_SCHEMA
            || self.organization_id != frame.organization_id
            || self.project_id != frame.project_id
            || self.parent_workflow_run_id != frame.workflow_run_id
            || self.parent_plan_revision_id != frame.plan_revision_id
            || self.parent_plan_digest != frame.plan_digest
            || self.region_step_id != frame.region_step_id
            || self.ordinal != frame.ordinal
            || self.frame_digest != frame.frame_digest
            || self.child_workflow_definition_id != frame.child_workflow_definition_id
            || self.child_workflow_revision_id != frame.child_workflow_revision_id
            || self.child_workflow_digest != frame.child_workflow_digest
            || self.child_workflow_run_id != frame.child_workflow_run_id()
            || self.child_workflow_goal_id != frame.child_workflow_goal_id()
            || self.child_operation_id.as_uuid() != self.child_workflow_run_id.as_uuid()
            || self.child_plan_revision_id != frame.child_plan_revision_id()
            || self.child_input_digest != frame.child_input_digest
        {
            return Err("Workflow composite child reference authority drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
            "Workflow composite child reference",
        )?;
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowCompositeResumeDigestBody<'a> {
    schema: &'a str,
    workflow_run_id: WorkflowRunId,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    frame_digest: &'a Sha256Digest,
    resolution: &'a WorkflowCompositeFrameResolution,
}
