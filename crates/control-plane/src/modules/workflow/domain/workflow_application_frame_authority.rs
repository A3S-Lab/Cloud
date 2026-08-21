use super::{WorkflowCompositeFrame, WorkflowPlan, WorkflowRunInput};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const WORKFLOW_APPLICATION_FRAME_AUTHORITY_SCHEMA: &str =
    "cloud.workflow.application-frame-authority.v1";
const WORKFLOW_APPLICATION_ROOT_PATH_SCHEMA: &str = "cloud.workflow.application-root-path.v1";
const WORKFLOW_APPLICATION_LOGICAL_PATH_SCHEMA: &str =
    "cloud.workflow.application-logical-frame-path.v1";
const WORKFLOW_APPLICATION_EXECUTION_PATH_SCHEMA: &str =
    "cloud.workflow.application-execution-frame-path.v1";
const WORKFLOW_APPLICATION_ANSWER_STEP_SCHEMA: &str =
    "cloud.workflow.application-frame-answer-step.v1";
const WORKFLOW_APPLICATION_FRAME_AUTHORITY_MAX_BYTES: usize = 8 * 1024;
const WORKFLOW_COMPOSITE_CHILD_ID_VERSION: &str = "cloud.workflow.composite-child.v1";

/// Immutable authority by which one composite child WorkflowRun projects
/// repeated Answer frames into the root Application invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowApplicationFrameAuthority {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_workflow_run_id: WorkflowRunId,
    pub parent_workflow_run_id: WorkflowRunId,
    pub parent_plan_revision_id: PlanRevisionId,
    pub parent_plan_digest: Sha256Digest,
    pub parent_execution_path_digest: Sha256Digest,
    pub region_step_id: String,
    pub frame_ordinal: u32,
    pub frame_digest: Sha256Digest,
    pub child_workflow_run_id: WorkflowRunId,
    pub child_workflow_definition_id: WorkflowDefinitionId,
    pub child_workflow_revision_id: WorkflowRevisionId,
    pub child_workflow_digest: Sha256Digest,
    pub logical_path_digest: Sha256Digest,
    pub execution_path_digest: Sha256Digest,
}

impl WorkflowApplicationFrameAuthority {
    pub fn from_parent(
        parent: &WorkflowRunInput,
        frame: &WorkflowCompositeFrame,
    ) -> Result<Option<Self>, String> {
        parent.validate()?;
        let Some(projection) = parent.application_projection.as_ref() else {
            return Ok(None);
        };
        projection.validate(&parent.plan)?;
        if !projection.supports_application_frames() {
            return Ok(None);
        }
        let (application_workflow_run_id, parent_execution_path_digest) = match projection
            .frame_authority
            .as_ref()
        {
            Some(authority) if !projection.projects_application_lifecycle() => {
                if authority.organization_id != parent.organization_id
                    || authority.project_id != parent.project_id
                {
                    return Err("Workflow Application frame parent tenant authority drifted".into());
                }
                (
                    authority.application_workflow_run_id,
                    authority.execution_path_digest.clone(),
                )
            }
            None if projection.projects_application_lifecycle() => {
                (parent.workflow_run_id, root_path_digest()?)
            }
            _ => {
                return Err(
                    "Workflow Application frame parent lost its root or nested authority".into(),
                )
            }
        };
        if frame.workflow_run_id != parent.workflow_run_id
            || frame.plan_revision_id != parent.plan_revision_id
            || frame.plan_digest != parent.plan_digest
        {
            return Err("Workflow Application frame drifted from its parent Run and Plan".into());
        }
        let logical_path_digest = logical_path_digest(
            &parent_execution_path_digest,
            frame.plan_revision_id,
            &frame.plan_digest,
            &frame.region_step_id,
            frame.child_workflow_definition_id,
            frame.child_workflow_revision_id,
            &frame.child_workflow_digest,
        )?;
        let execution_path_digest =
            execution_path_digest(&logical_path_digest, frame.ordinal, &frame.frame_digest)?;
        let value = Self {
            schema: WORKFLOW_APPLICATION_FRAME_AUTHORITY_SCHEMA.into(),
            organization_id: frame.organization_id,
            project_id: frame.project_id,
            application_workflow_run_id,
            parent_workflow_run_id: frame.workflow_run_id,
            parent_plan_revision_id: frame.plan_revision_id,
            parent_plan_digest: frame.plan_digest.clone(),
            parent_execution_path_digest,
            region_step_id: frame.region_step_id.clone(),
            frame_ordinal: frame.ordinal,
            frame_digest: frame.frame_digest.clone(),
            child_workflow_run_id: frame.child_workflow_run_id(),
            child_workflow_definition_id: frame.child_workflow_definition_id,
            child_workflow_revision_id: frame.child_workflow_revision_id,
            child_workflow_digest: frame.child_workflow_digest.clone(),
            logical_path_digest,
            execution_path_digest,
        };
        value.validate_for_frame(frame)?;
        Ok(Some(value))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_APPLICATION_FRAME_AUTHORITY_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_workflow_run_id.as_uuid().is_nil()
            || self.parent_workflow_run_id.as_uuid().is_nil()
            || self.parent_plan_revision_id.as_uuid().is_nil()
            || self.child_workflow_run_id.as_uuid().is_nil()
            || self.child_workflow_definition_id.as_uuid().is_nil()
            || self.child_workflow_revision_id.as_uuid().is_nil()
        {
            return Err("Workflow Application frame authority identities are invalid".into());
        }
        super::validation::validate_identifier(
            "Workflow Application frame region step",
            &self.region_step_id,
        )?;
        for digest in [
            &self.parent_plan_digest,
            &self.parent_execution_path_digest,
            &self.frame_digest,
            &self.child_workflow_digest,
            &self.logical_path_digest,
            &self.execution_path_digest,
        ] {
            if Sha256Digest::parse(digest.as_str())? != *digest {
                return Err("Workflow Application frame authority digest is invalid".into());
            }
        }
        if self.child_workflow_run_id
            != child_workflow_run_id(self.parent_workflow_run_id, &self.frame_digest)
            || logical_path_digest(
                &self.parent_execution_path_digest,
                self.parent_plan_revision_id,
                &self.parent_plan_digest,
                &self.region_step_id,
                self.child_workflow_definition_id,
                self.child_workflow_revision_id,
                &self.child_workflow_digest,
            )? != self.logical_path_digest
            || execution_path_digest(
                &self.logical_path_digest,
                self.frame_ordinal,
                &self.frame_digest,
            )? != self.execution_path_digest
        {
            return Err("Workflow Application frame authority derivation drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_APPLICATION_FRAME_AUTHORITY_MAX_BYTES,
            "Workflow Application frame authority",
        )?;
        Ok(())
    }

    pub fn validate_for_frame(&self, frame: &WorkflowCompositeFrame) -> Result<(), String> {
        self.validate()?;
        if self.organization_id != frame.organization_id
            || self.project_id != frame.project_id
            || self.parent_workflow_run_id != frame.workflow_run_id
            || self.parent_plan_revision_id != frame.plan_revision_id
            || self.parent_plan_digest != frame.plan_digest
            || self.region_step_id != frame.region_step_id
            || self.frame_ordinal != frame.ordinal
            || self.frame_digest != frame.frame_digest
            || self.child_workflow_run_id != frame.child_workflow_run_id()
            || self.child_workflow_definition_id != frame.child_workflow_definition_id
            || self.child_workflow_revision_id != frame.child_workflow_revision_id
            || self.child_workflow_digest != frame.child_workflow_digest
        {
            return Err(
                "Workflow Application frame authority does not match its exact frame".into(),
            );
        }
        Ok(())
    }

    pub fn validate_for_child(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_run_id: WorkflowRunId,
        plan: &WorkflowPlan,
    ) -> Result<(), String> {
        self.validate()?;
        plan.validate()?;
        if self.organization_id != organization_id
            || self.project_id != project_id
            || self.child_workflow_run_id != workflow_run_id
            || self.child_workflow_definition_id != plan.workflow_definition_id
            || self.child_workflow_revision_id != plan.workflow_revision_id
            || self.child_workflow_digest != plan.workflow_digest
        {
            return Err(
                "Workflow Application frame authority drifted from its child Run and Plan".into(),
            );
        }
        Ok(())
    }

    pub fn answer_effect_step_id(&self, step_id: &str) -> Result<String, String> {
        self.validate()?;
        super::validation::validate_identifier("Workflow Application Answer step", step_id)?;
        let bytes = canonical_json_bounded(
            &serde_json::json!({
                "schema": WORKFLOW_APPLICATION_ANSWER_STEP_SCHEMA,
                "logicalPathDigest": self.logical_path_digest,
                "stepId": step_id,
            }),
            2 * 1024,
            "Workflow Application frame Answer step identity",
        )?;
        Ok(format!(
            "frame-answer-{}",
            Uuid::new_v5(&Uuid::NAMESPACE_OID, &bytes)
        ))
    }
}

fn root_path_digest() -> Result<Sha256Digest, String> {
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        &serde_json::json!({"schema": WORKFLOW_APPLICATION_ROOT_PATH_SCHEMA}),
        512,
        "Workflow Application root path",
    )?))
}

#[allow(clippy::too_many_arguments)]
fn logical_path_digest(
    parent_execution_path_digest: &Sha256Digest,
    parent_plan_revision_id: PlanRevisionId,
    parent_plan_digest: &Sha256Digest,
    region_step_id: &str,
    child_workflow_definition_id: WorkflowDefinitionId,
    child_workflow_revision_id: WorkflowRevisionId,
    child_workflow_digest: &Sha256Digest,
) -> Result<Sha256Digest, String> {
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        &serde_json::json!({
            "schema": WORKFLOW_APPLICATION_LOGICAL_PATH_SCHEMA,
            "parentExecutionPathDigest": parent_execution_path_digest,
            "parentPlanRevisionId": parent_plan_revision_id,
            "parentPlanDigest": parent_plan_digest,
            "regionStepId": region_step_id,
            "childWorkflowDefinitionId": child_workflow_definition_id,
            "childWorkflowRevisionId": child_workflow_revision_id,
            "childWorkflowDigest": child_workflow_digest,
        }),
        4 * 1024,
        "Workflow Application logical frame path",
    )?))
}

fn execution_path_digest(
    logical_path_digest: &Sha256Digest,
    frame_ordinal: u32,
    frame_digest: &Sha256Digest,
) -> Result<Sha256Digest, String> {
    Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
        &serde_json::json!({
            "schema": WORKFLOW_APPLICATION_EXECUTION_PATH_SCHEMA,
            "logicalPathDigest": logical_path_digest,
            "frameOrdinal": frame_ordinal,
            "frameDigest": frame_digest,
        }),
        2 * 1024,
        "Workflow Application execution frame path",
    )?))
}

fn child_workflow_run_id(
    parent_workflow_run_id: WorkflowRunId,
    frame_digest: &Sha256Digest,
) -> WorkflowRunId {
    let identity = format!("{WORKFLOW_COMPOSITE_CHILD_ID_VERSION}:{frame_digest}");
    WorkflowRunId::from_uuid(Uuid::new_v5(
        &parent_workflow_run_id.as_uuid(),
        identity.as_bytes(),
    ))
}
