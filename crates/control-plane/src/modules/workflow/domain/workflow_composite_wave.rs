use super::workflow_composite_frame::validate_plan_bindings;
use super::{
    WorkflowCompositeFrame, WorkflowCompositeFrameMode, WorkflowCompositeFrameRequest,
    WorkflowCompositeFrameResolution, WorkflowCompositeFrameResult, WorkflowCompositeRegionPolicy,
    WorkflowCompositeRegionResultRequest, WorkflowCompositeRegions, WorkflowIterationFailureMode,
    WorkflowPlan, WorkflowVariableContract, WorkflowVariableDefaults,
    WORKFLOW_COMPOSITE_FRAME_MAX_BYTES, WORKFLOW_ITERATION_MAX_CONCURRENCY,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest, WorkflowRunId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const WORKFLOW_COMPOSITE_WAVE_HOOK_SCHEMA: &str = "cloud.workflow.composite-wave-hook.v1";
pub const WORKFLOW_COMPOSITE_WAVE_RESUME_SCHEMA: &str = "cloud.workflow.composite-wave-resume.v1";
pub const WORKFLOW_COMPOSITE_WAVE_MAX_BYTES: usize =
    WORKFLOW_COMPOSITE_FRAME_MAX_BYTES * (WORKFLOW_ITERATION_MAX_CONCURRENCY as usize + 1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCompositeWaveRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub region_step_id: String,
    pub first_ordinal: u32,
    pub effective_inputs: Vec<Value>,
    pub available_variables: BTreeMap<String, Value>,
}

/// Immutable authority for one bounded wave of Iteration children.
///
/// Common variable material is stored once and each exact child frame is
/// reconstructed from the pinned Run contracts. This keeps the hook bounded
/// without weakening per-frame identities or replay validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeWaveHookMetadata {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub region_step_id: String,
    pub first_ordinal: u32,
    pub effective_inputs: Vec<Value>,
    pub available_variables: BTreeMap<String, Value>,
    pub wave_digest: Sha256Digest,
}

impl WorkflowCompositeWaveHookMetadata {
    pub fn new(
        request: WorkflowCompositeWaveRequest,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: WORKFLOW_COMPOSITE_WAVE_HOOK_SCHEMA.into(),
            organization_id: request.organization_id,
            project_id: request.project_id,
            workflow_run_id: request.workflow_run_id,
            plan_revision_id: request.plan_revision_id,
            plan_digest: request.plan_digest,
            region_step_id: request.region_step_id,
            first_ordinal: request.first_ordinal,
            effective_inputs: request.effective_inputs,
            available_variables: request.available_variables,
            wave_digest: Sha256Digest::from_bytes(&[]),
        };
        value.wave_digest = value.compute_digest()?;
        value.validate(plan, regions, variables, defaults)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<(), String> {
        self.validated_frames(plan, regions, variables, defaults)
            .map(|_| ())
    }

    pub fn frames(
        &self,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<Vec<WorkflowCompositeFrame>, String> {
        self.validated_frames(plan, regions, variables, defaults)
    }

    pub const fn frame_count(&self) -> usize {
        self.effective_inputs.len()
    }

    pub fn last_ordinal(&self) -> Result<u32, String> {
        let frame_count = u32::try_from(self.frame_count())
            .map_err(|_| "Workflow composite wave frame count overflowed".to_owned())?;
        let last_offset = frame_count
            .checked_sub(1)
            .ok_or_else(|| "Workflow composite wave has no frames".to_owned())?;
        self.first_ordinal
            .checked_add(last_offset)
            .ok_or_else(|| "Workflow composite wave ordinal overflowed".to_owned())
    }

    pub fn flow_hook_id(&self) -> String {
        format!(
            "workflow-composite-wave:{}:{}:{}",
            self.region_step_id,
            self.first_ordinal,
            self.effective_inputs.len()
        )
    }

    pub fn flow_hook_token(&self) -> String {
        format!(
            "workflow-composite-wave:{}:{}:{}",
            self.workflow_run_id,
            self.flow_hook_id(),
            self.wave_digest
        )
    }

    fn validated_frames(
        &self,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<Vec<WorkflowCompositeFrame>, String> {
        if self.schema != WORKFLOW_COMPOSITE_WAVE_HOOK_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.region_step_id.is_empty()
            || self.region_step_id.len() > 96
        {
            return Err("Workflow composite wave authority is invalid".into());
        }
        validate_plan_bindings(plan, &self.plan_digest, regions, variables)?;
        let policy = match regions.resolve(&self.region_step_id) {
            Some(WorkflowCompositeRegionPolicy::Iteration(policy)) => policy,
            _ => return Err("Workflow composite wave requires an Iteration policy".into()),
        };
        let frame_count = u32::try_from(self.effective_inputs.len())
            .map_err(|_| "Workflow composite wave frame count overflowed".to_owned())?;
        if policy.maximum_concurrency <= 1
            || frame_count == 0
            || frame_count > policy.maximum_concurrency
            || !self
                .first_ordinal
                .is_multiple_of(policy.maximum_concurrency)
        {
            return Err("Workflow composite wave violates its immutable concurrency bound".into());
        }
        let end = self
            .first_ordinal
            .checked_add(frame_count)
            .ok_or_else(|| "Workflow composite wave ordinal overflowed".to_owned())?;
        if end > policy.maximum_items {
            return Err("Workflow composite wave exceeds its immutable item bound".into());
        }

        let frames = self
            .effective_inputs
            .iter()
            .enumerate()
            .map(|(offset, effective_input)| {
                let offset = u32::try_from(offset)
                    .map_err(|_| "Workflow composite wave offset overflowed".to_owned())?;
                let ordinal = self
                    .first_ordinal
                    .checked_add(offset)
                    .ok_or_else(|| "Workflow composite wave ordinal overflowed".to_owned())?;
                WorkflowCompositeFrame::open(
                    WorkflowCompositeFrameRequest {
                        organization_id: self.organization_id,
                        project_id: self.project_id,
                        workflow_run_id: self.workflow_run_id,
                        plan_revision_id: self.plan_revision_id,
                        plan_digest: self.plan_digest.clone(),
                        region_step_id: self.region_step_id.clone(),
                        ordinal,
                        effective_input: effective_input.clone(),
                        available_variables: self.available_variables.clone(),
                    },
                    plan,
                    regions,
                    variables,
                    defaults,
                )
            })
            .collect::<Result<Vec<_>, String>>()?;
        if self.wave_digest != self.compute_digest()? {
            return Err("Workflow composite wave digest drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_WAVE_MAX_BYTES,
            "Workflow composite wave hook metadata",
        )?;
        Ok(frames)
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
            &WorkflowCompositeWaveDigestBody {
                schema: &self.schema,
                organization_id: self.organization_id,
                project_id: self.project_id,
                workflow_run_id: self.workflow_run_id,
                plan_revision_id: self.plan_revision_id,
                plan_digest: &self.plan_digest,
                region_step_id: &self.region_step_id,
                first_ordinal: self.first_ordinal,
                effective_inputs: &self.effective_inputs,
                available_variables: &self.available_variables,
            },
            WORKFLOW_COMPOSITE_WAVE_MAX_BYTES,
            "Workflow composite wave digest body",
        )?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowCompositeWaveFrameResolution {
    Completed {
        ordinal: u32,
        frame_digest: Sha256Digest,
        result: WorkflowCompositeFrameResult,
    },
    Failed {
        ordinal: u32,
        frame_digest: Sha256Digest,
        error: String,
        primary: bool,
    },
}

impl WorkflowCompositeWaveFrameResolution {
    pub fn completed(frame: &WorkflowCompositeFrame, result: WorkflowCompositeFrameResult) -> Self {
        Self::Completed {
            ordinal: frame.ordinal,
            frame_digest: frame.frame_digest.clone(),
            result,
        }
    }

    pub fn failed(frame: &WorkflowCompositeFrame, error: impl Into<String>) -> Self {
        Self::Failed {
            ordinal: frame.ordinal,
            frame_digest: frame.frame_digest.clone(),
            error: error.into(),
            primary: true,
        }
    }

    pub fn cancelled_after_primary_failure(frame: &WorkflowCompositeFrame) -> Self {
        Self::Failed {
            ordinal: frame.ordinal,
            frame_digest: frame.frame_digest.clone(),
            error: "Workflow child was cancelled after another frame failed".into(),
            primary: false,
        }
    }

    pub const fn ordinal(&self) -> u32 {
        match self {
            Self::Completed { ordinal, .. } | Self::Failed { ordinal, .. } => *ordinal,
        }
    }

    pub const fn is_primary_failure(&self) -> bool {
        matches!(self, Self::Failed { primary: true, .. })
    }

    pub fn primary_failure(&self) -> Option<(u32, &str)> {
        match self {
            Self::Failed {
                ordinal,
                error,
                primary: true,
                ..
            } => Some((*ordinal, error)),
            _ => None,
        }
    }

    fn bind(
        &self,
        frame: &WorkflowCompositeFrame,
    ) -> Result<WorkflowCompositeFrameResolution, String> {
        let (ordinal, frame_digest) = match self {
            Self::Completed {
                ordinal,
                frame_digest,
                ..
            }
            | Self::Failed {
                ordinal,
                frame_digest,
                ..
            } => (*ordinal, frame_digest),
        };
        if ordinal != frame.ordinal || frame_digest != &frame.frame_digest {
            return Err("Workflow composite wave frame resolution authority drifted".into());
        }
        Ok(match self {
            Self::Completed { result, .. } => {
                WorkflowCompositeFrameResolution::completed(frame.clone(), result.clone())
            }
            Self::Failed { error, .. } => {
                WorkflowCompositeFrameResolution::failed(frame.clone(), error.clone())
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeWaveResumePayload {
    pub schema: String,
    pub workflow_run_id: WorkflowRunId,
    pub flow_run_id: String,
    pub flow_hook_id: String,
    pub wave_digest: Sha256Digest,
    pub resolutions: Vec<WorkflowCompositeWaveFrameResolution>,
    pub payload_digest: Sha256Digest,
}

impl WorkflowCompositeWaveResumePayload {
    pub fn new(
        metadata: &WorkflowCompositeWaveHookMetadata,
        mut resolutions: Vec<WorkflowCompositeWaveFrameResolution>,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<Self, String> {
        resolutions.sort_by_key(WorkflowCompositeWaveFrameResolution::ordinal);
        let mut value = Self {
            schema: WORKFLOW_COMPOSITE_WAVE_RESUME_SCHEMA.into(),
            workflow_run_id: metadata.workflow_run_id,
            flow_run_id: metadata.workflow_run_id.to_string(),
            flow_hook_id: metadata.flow_hook_id(),
            wave_digest: metadata.wave_digest.clone(),
            resolutions,
            payload_digest: Sha256Digest::from_bytes(&[]),
        };
        value.payload_digest = value.compute_digest()?;
        value.validate(metadata, plan, regions, variables, defaults)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        metadata: &WorkflowCompositeWaveHookMetadata,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<(), String> {
        if self.schema != WORKFLOW_COMPOSITE_WAVE_RESUME_SCHEMA
            || self.workflow_run_id != metadata.workflow_run_id
            || self.flow_run_id != metadata.workflow_run_id.to_string()
            || self.flow_hook_id != metadata.flow_hook_id()
            || self.wave_digest != metadata.wave_digest
        {
            return Err("Workflow composite wave resume authority drifted".into());
        }
        self.bind_resolutions(metadata, plan, regions, variables, defaults)?;
        if self.payload_digest != self.compute_digest()? {
            return Err("Workflow composite wave resume digest drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_WAVE_MAX_BYTES,
            "Workflow composite wave resume payload",
        )?;
        Ok(())
    }

    pub fn frame_resolutions(
        &self,
        metadata: &WorkflowCompositeWaveHookMetadata,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<Vec<WorkflowCompositeFrameResolution>, String> {
        self.validate(metadata, plan, regions, variables, defaults)?;
        self.bind_resolutions(metadata, plan, regions, variables, defaults)
    }

    fn bind_resolutions(
        &self,
        metadata: &WorkflowCompositeWaveHookMetadata,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        defaults: Option<&WorkflowVariableDefaults>,
    ) -> Result<Vec<WorkflowCompositeFrameResolution>, String> {
        let frames = metadata.frames(plan, regions, variables, defaults)?;
        if frames.len() != self.resolutions.len() {
            return Err("Workflow composite wave resume frame count drifted".into());
        }
        let has_secondary_failure = self.resolutions.iter().any(|resolution| {
            matches!(
                resolution,
                WorkflowCompositeWaveFrameResolution::Failed { primary: false, .. }
            )
        });
        if has_secondary_failure {
            let permits_sibling_cancellation = matches!(
                regions.resolve(&metadata.region_step_id),
                Some(WorkflowCompositeRegionPolicy::Iteration(policy))
                    if policy.failure_mode == WorkflowIterationFailureMode::Terminate
            );
            if !permits_sibling_cancellation
                || !self
                    .resolutions
                    .iter()
                    .any(WorkflowCompositeWaveFrameResolution::is_primary_failure)
            {
                return Err(
                    "Workflow composite wave secondary failure lost its primary failure authority"
                        .into(),
                );
            }
        }
        let request = WorkflowCompositeRegionResultRequest {
            organization_id: metadata.organization_id,
            project_id: metadata.project_id,
            workflow_run_id: metadata.workflow_run_id,
            plan_revision_id: metadata.plan_revision_id,
            plan_digest: metadata.plan_digest.clone(),
            region_step_id: metadata.region_step_id.clone(),
        };
        frames
            .iter()
            .zip(&self.resolutions)
            .map(|(frame, resolution)| {
                let bound = resolution.bind(frame)?;
                bound.validate(
                    &request,
                    plan,
                    regions,
                    variables,
                    WorkflowCompositeFrameMode::Iteration,
                )?;
                Ok(bound)
            })
            .collect()
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
            &WorkflowCompositeWaveResumeDigestBody {
                schema: &self.schema,
                workflow_run_id: self.workflow_run_id,
                flow_run_id: &self.flow_run_id,
                flow_hook_id: &self.flow_hook_id,
                wave_digest: &self.wave_digest,
                resolutions: &self.resolutions,
            },
            WORKFLOW_COMPOSITE_WAVE_MAX_BYTES,
            "Workflow composite wave resume digest body",
        )?))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowCompositeWaveDigestBody<'a> {
    schema: &'a str,
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    plan_revision_id: PlanRevisionId,
    plan_digest: &'a Sha256Digest,
    region_step_id: &'a str,
    first_ordinal: u32,
    effective_inputs: &'a [Value],
    available_variables: &'a BTreeMap<String, Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowCompositeWaveResumeDigestBody<'a> {
    schema: &'a str,
    workflow_run_id: WorkflowRunId,
    flow_run_id: &'a str,
    flow_hook_id: &'a str,
    wave_digest: &'a Sha256Digest,
    resolutions: &'a [WorkflowCompositeWaveFrameResolution],
}
