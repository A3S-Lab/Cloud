use super::workflow_composite_frame::validate_plan_bindings;
use super::workflow_variable_materialization::lookup_workflow_variable_path;
use super::{
    WorkflowCompositeFrame, WorkflowCompositeFrameMode, WorkflowCompositeFrameResult,
    WorkflowCompositeRegionPolicy, WorkflowCompositeRegions, WorkflowIterationFailureMode,
    WorkflowPlan, WorkflowVariableContract, WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest, WorkflowRunId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const WORKFLOW_COMPOSITE_REGION_RESULT_SCHEMA: &str =
    "cloud.workflow.composite-region-result.v1";
pub const WORKFLOW_COMPOSITE_REGION_RESULT_MAX_BYTES: usize = WORKFLOW_COMPOSITE_FRAME_MAX_BYTES;
const WORKFLOW_COMPOSITE_FRAME_ERROR_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCompositeRegionResultRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub region_step_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowCompositeFrameResolution {
    Completed {
        frame: WorkflowCompositeFrame,
        result: WorkflowCompositeFrameResult,
    },
    Failed {
        frame: WorkflowCompositeFrame,
        error: String,
    },
}

impl WorkflowCompositeFrameResolution {
    pub fn completed(frame: WorkflowCompositeFrame, result: WorkflowCompositeFrameResult) -> Self {
        Self::Completed { frame, result }
    }

    pub fn failed(frame: WorkflowCompositeFrame, error: impl Into<String>) -> Self {
        Self::Failed {
            frame,
            error: error.into(),
        }
    }

    pub const fn frame(&self) -> &WorkflowCompositeFrame {
        match self {
            Self::Completed { frame, .. } | Self::Failed { frame, .. } => frame,
        }
    }

    pub const fn ordinal(&self) -> u32 {
        self.frame().ordinal
    }

    fn child_output(&self) -> Option<&Value> {
        match self {
            Self::Completed { result, .. } => Some(&result.child_output),
            Self::Failed { .. } => None,
        }
    }

    pub(super) fn validate(
        &self,
        request: &WorkflowCompositeRegionResultRequest,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        mode: WorkflowCompositeFrameMode,
    ) -> Result<(), String> {
        let frame = self.frame();
        frame.validate(plan, regions, variables)?;
        if frame.organization_id != request.organization_id
            || frame.project_id != request.project_id
            || frame.workflow_run_id != request.workflow_run_id
            || frame.plan_revision_id != request.plan_revision_id
            || frame.plan_digest != request.plan_digest
            || frame.region_step_id != request.region_step_id
            || frame.mode != mode
        {
            return Err("Workflow composite frame resolution authority drifted".into());
        }
        match self {
            Self::Completed { frame, result } => result.validate(frame, variables),
            Self::Failed { error, .. } => validate_frame_error(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowCompositeRegionResult {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub variable_contract_digest: Sha256Digest,
    pub composite_regions_digest: Sha256Digest,
    pub region_step_id: String,
    pub mode: WorkflowCompositeFrameMode,
    pub expected_frames: u32,
    pub frames: Vec<WorkflowCompositeFrameResolution>,
    pub output: Value,
    pub output_digest: Sha256Digest,
    pub run_variable_updates: BTreeMap<String, Value>,
    pub exported_variables: BTreeMap<String, Value>,
    pub result_digest: Sha256Digest,
}

impl WorkflowCompositeRegionResult {
    pub fn resolve_iteration(
        request: WorkflowCompositeRegionResultRequest,
        expected_items: u32,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        frames: Vec<WorkflowCompositeFrameResolution>,
    ) -> Result<Self, String> {
        Self::resolve(
            request,
            WorkflowCompositeFrameMode::Iteration,
            expected_items,
            plan,
            regions,
            variables,
            frames,
        )
    }

    pub fn resolve_loop(
        request: WorkflowCompositeRegionResultRequest,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        frames: Vec<WorkflowCompositeFrameResolution>,
    ) -> Result<Self, String> {
        let expected_frames = u32::try_from(frames.len())
            .map_err(|_| "Workflow composite loop frame count overflowed".to_owned())?;
        Self::resolve(
            request,
            WorkflowCompositeFrameMode::Loop,
            expected_frames,
            plan,
            regions,
            variables,
            frames,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve(
        request: WorkflowCompositeRegionResultRequest,
        mode: WorkflowCompositeFrameMode,
        expected_frames: u32,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
        mut frames: Vec<WorkflowCompositeFrameResolution>,
    ) -> Result<Self, String> {
        validate_request(&request, plan, regions, variables)?;
        frames.sort_by_key(WorkflowCompositeFrameResolution::ordinal);
        let reduced = reduce(
            &request,
            mode,
            expected_frames,
            plan,
            regions,
            variables,
            &frames,
        )?;
        let output_bytes = canonical_json_bounded(
            &reduced.output,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow composite region output",
        )?;
        let mut value = Self {
            schema: WORKFLOW_COMPOSITE_REGION_RESULT_SCHEMA.into(),
            organization_id: request.organization_id,
            project_id: request.project_id,
            workflow_run_id: request.workflow_run_id,
            plan_revision_id: request.plan_revision_id,
            plan_digest: request.plan_digest,
            variable_contract_digest: variables.digest().clone(),
            composite_regions_digest: regions.digest().clone(),
            region_step_id: request.region_step_id,
            mode,
            expected_frames,
            frames,
            output: reduced.output,
            output_digest: Sha256Digest::from_bytes(&output_bytes),
            run_variable_updates: reduced.run_variable_updates,
            exported_variables: reduced.exported_variables,
            result_digest: Sha256Digest::from_bytes(&[]),
        };
        value.result_digest = value.compute_digest()?;
        value.validate(plan, regions, variables)?;
        Ok(value)
    }

    pub fn validate(
        &self,
        plan: &WorkflowPlan,
        regions: &WorkflowCompositeRegions,
        variables: &WorkflowVariableContract,
    ) -> Result<(), String> {
        let request = self.request();
        if self.schema != WORKFLOW_COMPOSITE_REGION_RESULT_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.region_step_id.is_empty()
            || self.region_step_id.len() > 96
        {
            return Err("Workflow composite region result authority is invalid".into());
        }
        validate_request(&request, plan, regions, variables)?;
        if self.variable_contract_digest != *variables.digest()
            || self.composite_regions_digest != *regions.digest()
        {
            return Err("Workflow composite region result contract authority drifted".into());
        }
        if !self
            .frames
            .windows(2)
            .all(|pair| pair[0].ordinal() < pair[1].ordinal())
        {
            return Err("Workflow composite region frames are not in unique ordinal order".into());
        }
        let reduced = reduce(
            &request,
            self.mode,
            self.expected_frames,
            plan,
            regions,
            variables,
            &self.frames,
        )?;
        if self.output != reduced.output
            || self.run_variable_updates != reduced.run_variable_updates
            || self.exported_variables != reduced.exported_variables
        {
            return Err("Workflow composite region reduction drifted".into());
        }
        let output_bytes = canonical_json_bounded(
            &self.output,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow composite region output",
        )?;
        if self.output_digest != Sha256Digest::from_bytes(&output_bytes)
            || self.result_digest != self.compute_digest()?
        {
            return Err("Workflow composite region result digest drifted".into());
        }
        canonical_json_bounded(
            self,
            WORKFLOW_COMPOSITE_REGION_RESULT_MAX_BYTES,
            "Workflow composite region result",
        )?;
        Ok(())
    }

    fn request(&self) -> WorkflowCompositeRegionResultRequest {
        WorkflowCompositeRegionResultRequest {
            organization_id: self.organization_id,
            project_id: self.project_id,
            workflow_run_id: self.workflow_run_id,
            plan_revision_id: self.plan_revision_id,
            plan_digest: self.plan_digest.clone(),
            region_step_id: self.region_step_id.clone(),
        }
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let body = WorkflowCompositeRegionResultDigestBody {
            schema: &self.schema,
            organization_id: self.organization_id,
            project_id: self.project_id,
            workflow_run_id: self.workflow_run_id,
            plan_revision_id: self.plan_revision_id,
            plan_digest: &self.plan_digest,
            variable_contract_digest: &self.variable_contract_digest,
            composite_regions_digest: &self.composite_regions_digest,
            region_step_id: &self.region_step_id,
            mode: self.mode,
            expected_frames: self.expected_frames,
            frames: &self.frames,
            output: &self.output,
            output_digest: &self.output_digest,
            run_variable_updates: &self.run_variable_updates,
            exported_variables: &self.exported_variables,
        };
        Ok(Sha256Digest::from_bytes(&canonical_json_bounded(
            &body,
            WORKFLOW_COMPOSITE_REGION_RESULT_MAX_BYTES,
            "Workflow composite region result digest body",
        )?))
    }
}

struct ReducedComposite {
    output: Value,
    run_variable_updates: BTreeMap<String, Value>,
    exported_variables: BTreeMap<String, Value>,
}

#[allow(clippy::too_many_arguments)]
fn reduce(
    request: &WorkflowCompositeRegionResultRequest,
    mode: WorkflowCompositeFrameMode,
    expected_frames: u32,
    plan: &WorkflowPlan,
    regions: &WorkflowCompositeRegions,
    variables: &WorkflowVariableContract,
    frames: &[WorkflowCompositeFrameResolution],
) -> Result<ReducedComposite, String> {
    let policy = regions.resolve(&request.region_step_id).ok_or_else(|| {
        "Workflow composite region result has no immutable region policy".to_owned()
    })?;
    let expected_mode = match policy {
        WorkflowCompositeRegionPolicy::Iteration(_) => WorkflowCompositeFrameMode::Iteration,
        WorkflowCompositeRegionPolicy::Loop(_) => WorkflowCompositeFrameMode::Loop,
    };
    if mode != expected_mode {
        return Err("Workflow composite region result mode drifted".into());
    }
    let frame_count = u32::try_from(frames.len())
        .map_err(|_| "Workflow composite region frame count overflowed".to_owned())?;
    match policy {
        WorkflowCompositeRegionPolicy::Iteration(iteration) => {
            if expected_frames > iteration.maximum_items || frame_count != expected_frames {
                return Err("Workflow iteration frame count violates its immutable bound".into());
            }
        }
        WorkflowCompositeRegionPolicy::Loop(loop_policy) => {
            if expected_frames == 0
                || expected_frames > loop_policy.maximum_iterations
                || frame_count != expected_frames
            {
                return Err("Workflow loop frame count violates its immutable bound".into());
            }
        }
    }

    let mut output = Vec::new();
    let mut run_variable_updates = BTreeMap::new();
    let mut exported_variables = BTreeMap::new();
    for (index, resolution) in frames.iter().enumerate() {
        let ordinal = u32::try_from(index)
            .map_err(|_| "Workflow composite frame ordinal overflowed".to_owned())?;
        if resolution.ordinal() != ordinal {
            return Err("Workflow composite frames are not contiguous from ordinal zero".into());
        }
        resolution.validate(request, plan, regions, variables, mode)?;
        match resolution {
            WorkflowCompositeFrameResolution::Completed { result, .. } => {
                output.push(result.child_output.clone());
                run_variable_updates.extend(result.run_variable_updates.clone());
                exported_variables.extend(result.exported_variables.clone());
            }
            WorkflowCompositeFrameResolution::Failed { error, .. } => match policy {
                WorkflowCompositeRegionPolicy::Iteration(iteration) => match iteration.failure_mode
                {
                    WorkflowIterationFailureMode::Terminate => {
                        return Err(format!(
                            "Workflow iteration frame {ordinal} failed: {error}"
                        ));
                    }
                    WorkflowIterationFailureMode::ContinueNull => output.push(Value::Null),
                    WorkflowIterationFailureMode::RemoveFailed => {}
                },
                WorkflowCompositeRegionPolicy::Loop(_) => {
                    return Err(format!("Workflow loop frame {ordinal} failed: {error}"));
                }
            },
        }
    }

    let output = match policy {
        WorkflowCompositeRegionPolicy::Iteration(_) => Value::Array(output),
        WorkflowCompositeRegionPolicy::Loop(loop_policy) => {
            let last = frames
                .last()
                .and_then(WorkflowCompositeFrameResolution::child_output)
                .ok_or_else(|| "Workflow loop has no successful terminal frame".to_owned())?;
            for (index, resolution) in frames.iter().enumerate() {
                let child_output = resolution.child_output().ok_or_else(|| {
                    "Workflow loop contains a failed frame before termination".to_owned()
                })?;
                let termination =
                    lookup_workflow_variable_path(child_output, &loop_policy.termination_path)
                        .and_then(Value::as_bool)
                        .ok_or_else(|| {
                            "Workflow loop termination path did not resolve to a boolean".to_owned()
                        })?;
                let is_last = index + 1 == frames.len();
                if termination != is_last {
                    return Err(if termination {
                        "Workflow loop retained frames after its termination condition".into()
                    } else {
                        "Workflow loop result was reduced before its termination condition".into()
                    });
                }
            }
            last.clone()
        }
    };
    Ok(ReducedComposite {
        output,
        run_variable_updates,
        exported_variables,
    })
}

fn validate_request(
    request: &WorkflowCompositeRegionResultRequest,
    plan: &WorkflowPlan,
    regions: &WorkflowCompositeRegions,
    variables: &WorkflowVariableContract,
) -> Result<(), String> {
    if request.organization_id.as_uuid().is_nil()
        || request.project_id.as_uuid().is_nil()
        || request.workflow_run_id.as_uuid().is_nil()
        || request.plan_revision_id.as_uuid().is_nil()
        || request.region_step_id.is_empty()
        || request.region_step_id.len() > 96
    {
        return Err("Workflow composite region result request authority is invalid".into());
    }
    validate_plan_bindings(plan, &request.plan_digest, regions, variables)?;
    if regions.resolve(&request.region_step_id).is_none() {
        return Err("Workflow composite region result references a missing region".into());
    }
    Ok(())
}

fn validate_frame_error(error: &str) -> Result<(), String> {
    if error.is_empty()
        || error.len() > WORKFLOW_COMPOSITE_FRAME_ERROR_MAX_BYTES
        || error.contains(['\0', '\r', '\n'])
    {
        return Err("Workflow composite frame failure is invalid".into());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowCompositeRegionResultDigestBody<'a> {
    schema: &'a str,
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    plan_revision_id: PlanRevisionId,
    plan_digest: &'a Sha256Digest,
    variable_contract_digest: &'a Sha256Digest,
    composite_regions_digest: &'a Sha256Digest,
    region_step_id: &'a str,
    mode: WorkflowCompositeFrameMode,
    expected_frames: u32,
    frames: &'a [WorkflowCompositeFrameResolution],
    output: &'a Value,
    output_digest: &'a Sha256Digest,
    run_variable_updates: &'a BTreeMap<String, Value>,
    exported_variables: &'a BTreeMap<String, Value>,
}
