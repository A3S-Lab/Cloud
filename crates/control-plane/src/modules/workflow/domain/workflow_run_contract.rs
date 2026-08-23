use super::entities::digest_payload_set;
use super::{
    validate_application_runtime_variable_contract, validate_runtime_variable_contract,
    validate_typed_projection_configurations, CapabilityType, WorkflowCompositeRegions,
    WorkflowDataSchema, WorkflowEdgeSpec, WorkflowPayload, WorkflowPayloadContent,
    WorkflowPayloadKind, WorkflowPlan, WorkflowPlanStep, WorkflowPolicy, WorkflowPolicyMode,
    WorkflowRunApplicationProjection, WorkflowStepConfiguration, WorkflowStepKind,
    WorkflowVariableContract, WorkflowVariableDefaults, WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES,
    WORKFLOW_GOAL_MAX_INPUT_BYTES, WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_PLAN_SCHEMA,
    WORKFLOW_PLAN_SCHEMA_V2, WORKFLOW_PLAN_SCHEMA_V3, WORKFLOW_PLAN_SCHEMA_V4,
    WORKFLOW_PLAN_SCHEMA_V5, WORKFLOW_PLAN_SCHEMA_V6, WORKFLOW_PLAN_SCHEMA_V7,
    WORKFLOW_PLAN_SCHEMA_V8, WORKFLOW_REVISION_MAX_PAYLOAD_BYTES,
    WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES, WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest,
    WorkflowGoalId, WorkflowRunId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod v16;

pub const WORKFLOW_RUN_INPUT_SCHEMA: &str = "cloud.workflow-run.input.v1";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION: &str = "cloud.workflow-run-runtime.v1";
pub const WORKFLOW_RUN_FLOW_NAME: &str = "cloud.workflow-run";
pub const WORKFLOW_RUN_FLOW_VERSION: &str = "1";
pub const WORKFLOW_RUN_INPUT_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const WORKFLOW_RUN_INPUT_SCHEMA_V2: &str = "cloud.workflow-run.input.v2";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2: &str = "cloud.workflow-run-runtime.v2";
pub const WORKFLOW_RUN_FLOW_VERSION_V2: &str = "2";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V3: &str = "cloud.workflow-run.input.v3";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3: &str = "cloud.workflow-run-runtime.v3";
pub const WORKFLOW_RUN_FLOW_VERSION_V3: &str = "3";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V4: &str = "cloud.workflow-run.input.v4";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4: &str = "cloud.workflow-run-runtime.v4";
pub const WORKFLOW_RUN_FLOW_VERSION_V4: &str = "4";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V5: &str = "cloud.workflow-run.input.v5";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5: &str = "cloud.workflow-run-runtime.v5";
pub const WORKFLOW_RUN_FLOW_VERSION_V5: &str = "5";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V6: &str = "cloud.workflow-run.input.v6";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6: &str = "cloud.workflow-run-runtime.v6";
pub const WORKFLOW_RUN_FLOW_VERSION_V6: &str = "6";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V7: &str = "cloud.workflow-run.input.v7";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7: &str = "cloud.workflow-run-runtime.v7";
pub const WORKFLOW_RUN_FLOW_VERSION_V7: &str = "7";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V8: &str = "cloud.workflow-run.input.v8";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8: &str = "cloud.workflow-run-runtime.v8";
pub const WORKFLOW_RUN_FLOW_VERSION_V8: &str = "8";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V9: &str = "cloud.workflow-run.input.v9";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9: &str = "cloud.workflow-run-runtime.v9";
pub const WORKFLOW_RUN_FLOW_VERSION_V9: &str = "9";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V10: &str = "cloud.workflow-run.input.v10";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10: &str = "cloud.workflow-run-runtime.v10";
pub const WORKFLOW_RUN_FLOW_VERSION_V10: &str = "10";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V11: &str = "cloud.workflow-run.input.v11";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11: &str = "cloud.workflow-run-runtime.v11";
pub const WORKFLOW_RUN_FLOW_VERSION_V11: &str = "11";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V12: &str = "cloud.workflow-run.input.v12";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12: &str = "cloud.workflow-run-runtime.v12";
pub const WORKFLOW_RUN_FLOW_VERSION_V12: &str = "12";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V13: &str = "cloud.workflow-run.input.v13";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13: &str = "cloud.workflow-run-runtime.v13";
pub const WORKFLOW_RUN_FLOW_VERSION_V13: &str = "13";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V14: &str = "cloud.workflow-run.input.v14";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14: &str = "cloud.workflow-run-runtime.v14";
pub const WORKFLOW_RUN_FLOW_VERSION_V14: &str = "14";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V15: &str = "cloud.workflow-run.input.v15";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15: &str = "cloud.workflow-run-runtime.v15";
pub const WORKFLOW_RUN_FLOW_VERSION_V15: &str = "15";
pub const WORKFLOW_RUN_INPUT_SCHEMA_V16: &str = "cloud.workflow-run.input.v16";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16: &str = "cloud.workflow-run-runtime.v16";
pub const WORKFLOW_RUN_FLOW_VERSION_V16: &str = "16";
pub const WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA: &str =
    "cloud.workflow-run.application-projection.v1";
pub const WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2: &str =
    "cloud.workflow-run.application-projection.v2";
pub const WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3: &str =
    "cloud.workflow-run.application-projection.v3";
pub const WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4: &str =
    "cloud.workflow-run.application-projection.v4";
pub const WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5: &str =
    "cloud.workflow-run.application-projection.v5";
/// Plan v2 plus worst-case JSON escaping of payload and variable ACL strings,
/// with four MiB reserved for the goal value, identities, and JSON framing.
pub const WORKFLOW_RUN_INPUT_MAX_BYTES_V2: usize = WORKFLOW_PLAN_MAX_BYTES
    + (2 * WORKFLOW_REVISION_MAX_PAYLOAD_BYTES)
    + (2 * WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES)
    + (2 * WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES)
    + (2 * WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES)
    + (4 * 1024 * 1024);
pub const WORKFLOW_RUN_OUTPUT_MAX_BYTES: usize = 256 * 1024;
pub const WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;
pub const WORKFLOW_RUN_MAX_TIMEOUT_SECONDS: u64 = 30 * 24 * 60 * 60;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedWorkflowPayload {
    pub kind: WorkflowPayloadKind,
    pub canonical_acl: String,
    pub digest: Sha256Digest,
}

impl ResolvedWorkflowPayload {
    pub fn from_payload(payload: &WorkflowPayload) -> Self {
        Self {
            kind: payload.kind(),
            canonical_acl: payload.canonical_acl().to_owned(),
            digest: payload.digest().clone(),
        }
    }

    pub fn restore(&self) -> Result<WorkflowPayload, String> {
        WorkflowPayload::restore(self.kind, &self.canonical_acl, self.digest.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedWorkflowVariableContract {
    pub canonical_acl: String,
    pub digest: Sha256Digest,
}

impl ResolvedWorkflowVariableContract {
    pub(crate) fn from_contract(contract: &WorkflowVariableContract) -> Self {
        Self {
            canonical_acl: contract.canonical_acl().to_owned(),
            digest: contract.digest().clone(),
        }
    }

    pub(crate) fn restore(&self) -> Result<WorkflowVariableContract, String> {
        WorkflowVariableContract::restore(&self.canonical_acl, self.digest.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedWorkflowVariableDefaults {
    pub canonical_acl: String,
    pub digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedWorkflowCompositeRegions {
    pub canonical_acl: String,
    pub digest: Sha256Digest,
}

impl ResolvedWorkflowCompositeRegions {
    pub(crate) fn from_regions(regions: &WorkflowCompositeRegions) -> Self {
        Self {
            canonical_acl: regions.canonical_acl().to_owned(),
            digest: regions.digest().clone(),
        }
    }

    pub(crate) fn restore(&self) -> Result<WorkflowCompositeRegions, String> {
        WorkflowCompositeRegions::restore(&self.canonical_acl, self.digest.as_str())
    }
}

impl ResolvedWorkflowVariableDefaults {
    pub(crate) fn from_defaults(defaults: &WorkflowVariableDefaults) -> Self {
        Self {
            canonical_acl: defaults.canonical_acl().to_owned(),
            digest: defaults.digest().clone(),
        }
    }

    pub(crate) fn restore(&self) -> Result<WorkflowVariableDefaults, String> {
        WorkflowVariableDefaults::restore(&self.canonical_acl, self.digest.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRunInput {
    pub schema: String,
    pub runtime_contract_revision: String,
    pub flow_workflow_name: String,
    pub flow_workflow_version: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_run_id: WorkflowRunId,
    pub workflow_goal_id: WorkflowGoalId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub plan: WorkflowPlan,
    pub goal_input: serde_json::Value,
    pub payloads: Vec<ResolvedWorkflowPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_contract: Option<ResolvedWorkflowVariableContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_defaults: Option<ResolvedWorkflowVariableDefaults>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite_regions: Option<ResolvedWorkflowCompositeRegions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_projection: Option<WorkflowRunApplicationProjection>,
    pub requested_at: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedWorkflowRunStep {
    pub plan: WorkflowPlanStep,
    pub configuration: WorkflowStepConfiguration,
    pub input_schema: WorkflowDataSchema,
    pub output_schema: WorkflowDataSchema,
    pub policy: Option<WorkflowPolicy>,
}

impl WorkflowRunInput {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        let maximum_bytes = if matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_INPUT_SCHEMA_V2
                | WORKFLOW_RUN_INPUT_SCHEMA_V3
                | WORKFLOW_RUN_INPUT_SCHEMA_V4
                | WORKFLOW_RUN_INPUT_SCHEMA_V5
                | WORKFLOW_RUN_INPUT_SCHEMA_V6
                | WORKFLOW_RUN_INPUT_SCHEMA_V7
                | WORKFLOW_RUN_INPUT_SCHEMA_V8
                | WORKFLOW_RUN_INPUT_SCHEMA_V9
                | WORKFLOW_RUN_INPUT_SCHEMA_V10
                | WORKFLOW_RUN_INPUT_SCHEMA_V11
                | WORKFLOW_RUN_INPUT_SCHEMA_V12
                | WORKFLOW_RUN_INPUT_SCHEMA_V13
                | WORKFLOW_RUN_INPUT_SCHEMA_V14
                | WORKFLOW_RUN_INPUT_SCHEMA_V15
                | WORKFLOW_RUN_INPUT_SCHEMA_V16
        ) {
            WORKFLOW_RUN_INPUT_MAX_BYTES_V2
        } else {
            WORKFLOW_RUN_INPUT_MAX_BYTES
        };
        canonical_json_bounded(self, maximum_bytes, "WorkflowRun input")
    }

    pub fn validate(&self) -> Result<(), String> {
        let (
            variable_contract,
            variable_defaults,
            composite_regions,
            composite_runtime,
            connector_runtime_capable,
        ) = {
            match (
                self.schema.as_str(),
                self.runtime_contract_revision.as_str(),
                self.flow_workflow_version.as_str(),
                self.plan.schema.as_str(),
                self.variable_contract.as_ref(),
                self.variable_defaults.as_ref(),
                self.composite_regions.as_ref(),
                self.application_projection.as_ref(),
            ) {
                (
                    WORKFLOW_RUN_INPUT_SCHEMA,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
                    WORKFLOW_RUN_FLOW_VERSION,
                    WORKFLOW_PLAN_SCHEMA,
                    None,
                    None,
                    None,
                    None,
                ) => (None, None, None, false, false),
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V2,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
                    WORKFLOW_RUN_FLOW_VERSION_V2,
                    WORKFLOW_PLAN_SCHEMA_V2,
                    Some(resolved),
                    defaults,
                    regions,
                    None,
                ) => {
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) => {}
                        (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    (Some(contract), defaults, regions, false, false)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V3,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3,
                    WORKFLOW_RUN_FLOW_VERSION_V3,
                    WORKFLOW_PLAN_SCHEMA_V2,
                    Some(resolved),
                    defaults,
                    Some(resolved_regions),
                    None,
                ) => {
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = resolved_regions.restore()?;
                    if self.plan.composite_regions_digest.as_ref() != Some(regions.digest()) {
                        return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        );
                    }
                    (Some(contract), defaults, Some(regions), true, false)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V4,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4,
                    WORKFLOW_RUN_FLOW_VERSION_V4,
                    WORKFLOW_PLAN_SCHEMA_V3,
                    Some(resolved),
                    defaults,
                    regions,
                    None,
                ) => {
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, false)
                }
                (
                    schema @ (WORKFLOW_RUN_INPUT_SCHEMA_V5
                    | WORKFLOW_RUN_INPUT_SCHEMA_V6
                    | WORKFLOW_RUN_INPUT_SCHEMA_V7
                    | WORKFLOW_RUN_INPUT_SCHEMA_V8
                    | WORKFLOW_RUN_INPUT_SCHEMA_V9),
                    runtime @ (WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8
                    | WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9),
                    flow @ (WORKFLOW_RUN_FLOW_VERSION_V5
                    | WORKFLOW_RUN_FLOW_VERSION_V6
                    | WORKFLOW_RUN_FLOW_VERSION_V7
                    | WORKFLOW_RUN_FLOW_VERSION_V8
                    | WORKFLOW_RUN_FLOW_VERSION_V9),
                    plan_schema @ (WORKFLOW_PLAN_SCHEMA_V2
                    | WORKFLOW_PLAN_SCHEMA_V3
                    | WORKFLOW_PLAN_SCHEMA_V4
                    | WORKFLOW_PLAN_SCHEMA_V5),
                    Some(resolved),
                    defaults,
                    regions,
                    None,
                ) if matches!(
                    (schema, runtime, flow),
                    (
                        WORKFLOW_RUN_INPUT_SCHEMA_V5,
                        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5,
                        WORKFLOW_RUN_FLOW_VERSION_V5
                    ) | (
                        WORKFLOW_RUN_INPUT_SCHEMA_V6,
                        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6,
                        WORKFLOW_RUN_FLOW_VERSION_V6
                    ) | (
                        WORKFLOW_RUN_INPUT_SCHEMA_V7,
                        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7,
                        WORKFLOW_RUN_FLOW_VERSION_V7
                    ) | (
                        WORKFLOW_RUN_INPUT_SCHEMA_V8,
                        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
                        WORKFLOW_RUN_FLOW_VERSION_V8
                    ) | (
                        WORKFLOW_RUN_INPUT_SCHEMA_V9,
                        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9,
                        WORKFLOW_RUN_FLOW_VERSION_V9
                    )
                ) && ((matches!(
                    plan_schema,
                    WORKFLOW_PLAN_SCHEMA_V2 | WORKFLOW_PLAN_SCHEMA_V3
                ) && matches!(
                    flow,
                    WORKFLOW_RUN_FLOW_VERSION_V5
                        | WORKFLOW_RUN_FLOW_VERSION_V6
                        | WORKFLOW_RUN_FLOW_VERSION_V8
                )) || (plan_schema == WORKFLOW_PLAN_SCHEMA_V4
                    && matches!(
                        flow,
                        WORKFLOW_RUN_FLOW_VERSION_V7 | WORKFLOW_RUN_FLOW_VERSION_V8
                    ))
                    || (plan_schema == WORKFLOW_PLAN_SCHEMA_V5
                        && flow == WORKFLOW_RUN_FLOW_VERSION_V9)) =>
                {
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    let connector_runtime_capable = matches!(
                        flow,
                        WORKFLOW_RUN_FLOW_VERSION_V5
                            | WORKFLOW_RUN_FLOW_VERSION_V6
                            | WORKFLOW_RUN_FLOW_VERSION_V8
                            | WORKFLOW_RUN_FLOW_VERSION_V9
                    );
                    (
                        Some(contract),
                        defaults,
                        regions,
                        composite_runtime,
                        connector_runtime_capable,
                    )
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V10,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10,
                    WORKFLOW_RUN_FLOW_VERSION_V10,
                    WORKFLOW_PLAN_SCHEMA_V2
                    | WORKFLOW_PLAN_SCHEMA_V3
                    | WORKFLOW_PLAN_SCHEMA_V4
                    | WORKFLOW_PLAN_SCHEMA_V5,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V11,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11,
                    WORKFLOW_RUN_FLOW_VERSION_V11,
                    WORKFLOW_PLAN_SCHEMA_V2
                    | WORKFLOW_PLAN_SCHEMA_V3
                    | WORKFLOW_PLAN_SCHEMA_V4
                    | WORKFLOW_PLAN_SCHEMA_V5,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V12,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12,
                    WORKFLOW_RUN_FLOW_VERSION_V12,
                    WORKFLOW_PLAN_SCHEMA_V2
                    | WORKFLOW_PLAN_SCHEMA_V3
                    | WORKFLOW_PLAN_SCHEMA_V4
                    | WORKFLOW_PLAN_SCHEMA_V5,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_application_runtime_variable_contract(
                        &contract,
                        defaults.as_ref(),
                        &self.plan,
                    )?;
                    application_projection.validate_variable_contract(&self.plan, &contract)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V13,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13,
                    WORKFLOW_RUN_FLOW_VERSION_V13,
                    WORKFLOW_PLAN_SCHEMA_V2
                    | WORKFLOW_PLAN_SCHEMA_V3
                    | WORKFLOW_PLAN_SCHEMA_V4
                    | WORKFLOW_PLAN_SCHEMA_V5,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4 =>
                {
                    application_projection.validate(&self.plan)?;
                    application_projection
                        .frame_authority
                        .as_ref()
                        .ok_or_else(|| {
                            "WorkflowRun Application frame projection lost its authority".to_owned()
                        })?
                        .validate_for_child(
                            self.organization_id,
                            self.project_id,
                            self.workflow_run_id,
                            &self.plan,
                        )?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V13,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13,
                    WORKFLOW_RUN_FLOW_VERSION_V13,
                    WORKFLOW_PLAN_SCHEMA_V2
                    | WORKFLOW_PLAN_SCHEMA_V3
                    | WORKFLOW_PLAN_SCHEMA_V4
                    | WORKFLOW_PLAN_SCHEMA_V5,
                    Some(resolved),
                    defaults,
                    Some(resolved_regions),
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    if application_projection.variable_step_ids.is_empty() {
                        validate_runtime_variable_contract(
                            &contract,
                            defaults.as_ref(),
                            &self.plan,
                        )?;
                    } else {
                        validate_application_runtime_variable_contract(
                            &contract,
                            defaults.as_ref(),
                            &self.plan,
                        )?;
                        application_projection.validate_variable_contract(&self.plan, &contract)?;
                    }
                    let regions = resolved_regions.restore()?;
                    if self.plan.composite_regions_digest.as_ref() != Some(regions.digest()) {
                        return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        );
                    }
                    (Some(contract), defaults, Some(regions), true, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V14,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
                    WORKFLOW_RUN_FLOW_VERSION_V14,
                    WORKFLOW_PLAN_SCHEMA_V6,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_application_runtime_variable_contract(
                        &contract,
                        defaults.as_ref(),
                        &self.plan,
                    )?;
                    application_projection.validate_variable_contract(&self.plan, &contract)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V14,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
                    WORKFLOW_RUN_FLOW_VERSION_V14,
                    WORKFLOW_PLAN_SCHEMA_V6,
                    Some(resolved),
                    defaults,
                    Some(resolved_regions),
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_application_runtime_variable_contract(
                        &contract,
                        defaults.as_ref(),
                        &self.plan,
                    )?;
                    application_projection.validate_variable_contract(&self.plan, &contract)?;
                    let regions = resolved_regions.restore()?;
                    if self.plan.composite_regions_digest.as_ref() != Some(regions.digest()) {
                        return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        );
                    }
                    (Some(contract), defaults, Some(regions), true, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V15,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15,
                    WORKFLOW_RUN_FLOW_VERSION_V15,
                    WORKFLOW_PLAN_SCHEMA_V7,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V15,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15,
                    WORKFLOW_RUN_FLOW_VERSION_V15,
                    WORKFLOW_PLAN_SCHEMA_V7,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_application_runtime_variable_contract(
                        &contract,
                        defaults.as_ref(),
                        &self.plan,
                    )?;
                    application_projection.validate_variable_contract(&self.plan, &contract)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V15,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15,
                    WORKFLOW_RUN_FLOW_VERSION_V15,
                    WORKFLOW_PLAN_SCHEMA_V7,
                    Some(resolved),
                    defaults,
                    regions,
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4 =>
                {
                    application_projection.validate(&self.plan)?;
                    application_projection
                        .frame_authority
                        .as_ref()
                        .ok_or_else(|| {
                            "WorkflowRun Application frame projection lost its authority".to_owned()
                        })?
                        .validate_for_child(
                            self.organization_id,
                            self.project_id,
                            self.workflow_run_id,
                            &self.plan,
                        )?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    validate_runtime_variable_contract(&contract, defaults.as_ref(), &self.plan)?;
                    let regions = regions
                        .map(ResolvedWorkflowCompositeRegions::restore)
                        .transpose()?;
                    match (
                        self.plan.composite_regions_digest.as_ref(),
                        regions.as_ref(),
                    ) {
                        (None, None) | (Some(_), Some(_)) => {}
                        _ => return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        ),
                    }
                    let composite_runtime = regions.is_some();
                    (Some(contract), defaults, regions, composite_runtime, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V15,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15,
                    WORKFLOW_RUN_FLOW_VERSION_V15,
                    WORKFLOW_PLAN_SCHEMA_V7,
                    Some(resolved),
                    defaults,
                    Some(resolved_regions),
                    Some(application_projection),
                ) if application_projection.schema
                    == WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5 =>
                {
                    application_projection.validate(&self.plan)?;
                    let contract = resolved.restore()?;
                    if self.plan.variable_contract_digest.as_ref() != Some(contract.digest()) {
                        return Err(
                            "WorkflowRun variable contract drifted from the PlanRevision".into(),
                        );
                    }
                    let defaults = defaults
                        .map(ResolvedWorkflowVariableDefaults::restore)
                        .transpose()?;
                    if application_projection.variable_step_ids.is_empty() {
                        validate_runtime_variable_contract(
                            &contract,
                            defaults.as_ref(),
                            &self.plan,
                        )?;
                    } else {
                        validate_application_runtime_variable_contract(
                            &contract,
                            defaults.as_ref(),
                            &self.plan,
                        )?;
                        application_projection.validate_variable_contract(&self.plan, &contract)?;
                    }
                    let regions = resolved_regions.restore()?;
                    if self.plan.composite_regions_digest.as_ref() != Some(regions.digest()) {
                        return Err(
                            "WorkflowRun composite region material drifted from the PlanRevision"
                                .into(),
                        );
                    }
                    (Some(contract), defaults, Some(regions), true, true)
                }
                (
                    WORKFLOW_RUN_INPUT_SCHEMA_V16,
                    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16,
                    WORKFLOW_RUN_FLOW_VERSION_V16,
                    WORKFLOW_PLAN_SCHEMA_V8,
                    Some(resolved),
                    defaults,
                    regions,
                    application_projection,
                ) => v16::validate(self, resolved, defaults, regions, application_projection)?,
                _ => {
                    return Err(
                        "WorkflowRun input, runtime, plan, and Flow versions are incompatible"
                            .into(),
                    )
                }
            }
        };
        if self.flow_workflow_name != WORKFLOW_RUN_FLOW_NAME
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_run_id.as_uuid().is_nil()
            || self.workflow_goal_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.deadline_at <= self.requested_at
        {
            return Err("WorkflowRun input authority or timeout contract is invalid".into());
        }
        self.plan.validate()?;
        if let Some(contract) = variable_contract.as_ref() {
            let workflow = self.plan.workflow_spec()?;
            if let Some(application) = self.application_projection.as_ref().filter(|projection| {
                matches!(
                    projection.schema.as_str(),
                    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
                        | WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
                ) && !projection.variable_step_ids.is_empty()
            }) {
                let application_ports = application
                    .variable_step_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>();
                contract.validate_graph_bindings_with_application_ports(
                    &workflow,
                    &application_ports,
                )?;
            } else {
                contract.validate_graph_bindings(&workflow)?;
            }
        }
        if let (Some(contract), Some(defaults)) =
            (variable_contract.as_ref(), variable_defaults.as_ref())
        {
            defaults.validate_contract(contract)?;
        }
        if let Some(regions) = composite_regions.as_ref() {
            regions.validate_plan(&self.plan)?;
        }
        let canonical_plan =
            canonical_json_bounded(&self.plan, WORKFLOW_PLAN_MAX_BYTES, "Workflow plan")?;
        if sha256_digest(&canonical_plan) != self.plan_digest.as_str() {
            return Err("WorkflowRun PlanRevision digest does not match its exact plan".into());
        }
        let canonical_input = canonical_json_bounded(
            &self.goal_input,
            WORKFLOW_GOAL_MAX_INPUT_BYTES,
            "WorkflowRun goal input",
        )?;
        if sha256_digest(&canonical_input) != self.plan.input_digest.as_str() {
            return Err("WorkflowRun goal input drifted from the PlanRevision input digest".into());
        }
        let restored = self.restore_payloads()?;
        if digest_payload_set(&restored)? != self.plan.workflow_payload_set_digest {
            return Err("WorkflowRun payload set drifted from the PlanRevision".into());
        }
        let resolved = resolve_steps(&self.plan, &restored)?;
        let has_connector = resolved.iter().any(|step| {
            step.plan.capability.as_ref().is_some_and(|capability| {
                capability.capability_type == CapabilityType::ConnectorRevision
            })
        });
        let connector_runtime_required = matches!(
            self.schema.as_str(),
            WORKFLOW_RUN_INPUT_SCHEMA_V5
                | WORKFLOW_RUN_INPUT_SCHEMA_V6
                | WORKFLOW_RUN_INPUT_SCHEMA_V8
                | WORKFLOW_RUN_INPUT_SCHEMA_V9
        );
        if (has_connector && !connector_runtime_capable)
            || (!has_connector && connector_runtime_required)
        {
            return Err(
                "WorkflowRun Connector runtime generation does not match its exact plan".into(),
            );
        }
        if let Some(contract) = variable_contract.as_ref() {
            validate_typed_projection_configurations(contract, &resolved)?;
        }
        for step in &resolved {
            validate_runtime_retry_policy(step)?;
            validate_runtime_default_output(step)?;
            let connector_step = step.plan.capability.as_ref().is_some_and(|capability| {
                capability.capability_type == CapabilityType::ConnectorRevision
            });
            if connector_step
                && (step.plan.kind != WorkflowStepKind::Service
                    || self.plan.environment_id.is_none())
            {
                return Err(format!(
                    "WorkflowRun Connector step {:?} requires Service kind and one exact environment",
                    step.plan.id
                ));
            }
            let supported = matches!(
                step.plan.kind,
                WorkflowStepKind::Input
                    | WorkflowStepKind::Transform
                    | WorkflowStepKind::Branch
                    | WorkflowStepKind::HumanDecision
                    | WorkflowStepKind::Execution
                    | WorkflowStepKind::Output
            ) || (composite_runtime
                && step.plan.kind == WorkflowStepKind::Subworkflow)
                || (connector_runtime_capable
                    && step.plan.kind == WorkflowStepKind::Service
                    && connector_step)
                || self
                    .application_projection
                    .as_ref()
                    .is_some_and(|projection| {
                        projection.is_variable_assignment_step(&step.plan.id)
                            && step.plan.kind == WorkflowStepKind::Service
                    });
            if !supported {
                return Err(format!(
                    "WorkflowRun runtime does not execute {} step {:?}",
                    step.plan.kind.as_str(),
                    step.plan.id
                ));
            }
            if step
                .policy
                .as_ref()
                .is_some_and(|policy| policy.mode != WorkflowPolicyMode::Static)
            {
                return Err(format!(
                    "WorkflowRun rejects recorded-choice policy on step {:?}",
                    step.plan.id
                ));
            }
            validate_branch_binding(step, &self.plan.edges)?;
        }
        self.canonical_bytes().map(|_| ())
    }

    pub fn resolved_steps(&self) -> Result<Vec<ResolvedWorkflowRunStep>, String> {
        let payloads = self.restore_payloads()?;
        resolve_steps(&self.plan, &payloads)
    }

    fn restore_payloads(&self) -> Result<Vec<WorkflowPayload>, String> {
        if self.payloads.is_empty() {
            return Err("WorkflowRun input has no resolved Workflow payloads".into());
        }
        let mut previous: Option<&Sha256Digest> = None;
        let mut restored = Vec::with_capacity(self.payloads.len());
        for payload in &self.payloads {
            if previous.is_some_and(|digest| digest >= &payload.digest) {
                return Err("WorkflowRun resolved payloads are not in unique digest order".into());
            }
            previous = Some(&payload.digest);
            restored.push(payload.restore()?);
        }
        Ok(restored)
    }
}

fn validate_runtime_default_output(step: &ResolvedWorkflowRunStep) -> Result<(), String> {
    let contract = step.plan.default_output.as_ref();
    let material = step
        .policy
        .as_ref()
        .and_then(|policy| policy.default_output.as_ref());
    match (contract, material) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(format!(
            "WorkflowRun step {:?} has default-output material without Plan authority",
            step.plan.id
        )),
        (Some(_), None) => Err(format!(
            "WorkflowRun step {:?} lost its immutable default-output material",
            step.plan.id
        )),
        (Some(contract), Some(material)) => {
            if step.plan.kind != WorkflowStepKind::Execution
                || material.port != contract.output_port.name
                || !contract
                    .output_port
                    .value_type
                    .matches_json_value(&material.value)
            {
                return Err(format!(
                    "WorkflowRun step {:?} default-output authority drifted",
                    step.plan.id
                ));
            }
            step.output_schema.validate_value(
                &material.value,
                &format!("WorkflowRun step {:?} default output", step.plan.id),
            )
        }
    }
}

pub(super) fn validate_runtime_retry_policy(step: &ResolvedWorkflowRunStep) -> Result<(), String> {
    let connector =
        step.plan.capability.as_ref().is_some_and(|capability| {
            capability.capability_type == CapabilityType::ConnectorRevision
        });
    let retry = step
        .policy
        .as_ref()
        .and_then(|policy| policy.retry.as_ref());
    match (connector, retry) {
        (true, Some(_)) | (false, None) => Ok(()),
        (true, None) => Err(format!(
            "WorkflowRun Connector step {:?} lost its immutable retry budget",
            step.plan.id
        )),
        (false, Some(_)) => Err(format!(
            "WorkflowRun step {:?} has a retry budget without an admitted provider runtime",
            step.plan.id
        )),
    }
}

fn resolve_steps(
    plan: &WorkflowPlan,
    payloads: &[WorkflowPayload],
) -> Result<Vec<ResolvedWorkflowRunStep>, String> {
    let by_digest = payloads
        .iter()
        .map(|payload| (payload.digest(), payload))
        .collect::<BTreeMap<_, _>>();
    let mut referenced = BTreeSet::new();
    let steps = plan
        .steps
        .iter()
        .map(|step| {
            let configuration = require_payload(
                &by_digest,
                &step.configuration_digest,
                WorkflowPayloadKind::Configuration,
                &step.id,
            )?;
            let WorkflowPayloadContent::Configuration(configuration) = configuration.content()
            else {
                return Err("WorkflowRun configuration payload content has the wrong kind".into());
            };
            if configuration.step_kind != step.kind {
                return Err(format!(
                    "WorkflowRun step {:?} configuration kind does not match its plan",
                    step.id
                ));
            }
            referenced.insert(step.configuration_digest.clone());

            let input_schema =
                require_schema(&by_digest, &step.input_schema_digest, &step.id, "input")?;
            referenced.insert(step.input_schema_digest.clone());
            let output_schema =
                require_schema(&by_digest, &step.output_schema_digest, &step.id, "output")?;
            referenced.insert(step.output_schema_digest.clone());
            let policy = step
                .policy_digest
                .as_ref()
                .map(|digest| {
                    let payload =
                        require_payload(&by_digest, digest, WorkflowPayloadKind::Policy, &step.id)?;
                    let WorkflowPayloadContent::Policy(policy) = payload.content() else {
                        return Err("WorkflowRun policy payload content has the wrong kind".into());
                    };
                    referenced.insert(digest.clone());
                    Ok::<WorkflowPolicy, String>(policy.clone())
                })
                .transpose()?;
            Ok(ResolvedWorkflowRunStep {
                plan: step.clone(),
                configuration: configuration.clone(),
                input_schema,
                output_schema,
                policy,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let stored = payloads
        .iter()
        .map(|payload| payload.digest().clone())
        .collect::<BTreeSet<_>>();
    if referenced != stored {
        return Err("WorkflowRun input must contain exactly the PlanRevision payloads".into());
    }
    Ok(steps)
}

fn require_payload<'a>(
    payloads: &'a BTreeMap<&Sha256Digest, &'a WorkflowPayload>,
    digest: &Sha256Digest,
    kind: WorkflowPayloadKind,
    step_id: &str,
) -> Result<&'a WorkflowPayload, String> {
    let payload = payloads.get(digest).copied().ok_or_else(|| {
        format!(
            "WorkflowRun step {step_id:?} is missing resolved {} payload {digest}",
            kind.as_str()
        )
    })?;
    if payload.kind() != kind {
        return Err(format!(
            "WorkflowRun step {step_id:?} resolves {digest} as {}, not {}",
            payload.kind().as_str(),
            kind.as_str()
        ));
    }
    Ok(payload)
}

fn require_schema(
    payloads: &BTreeMap<&Sha256Digest, &WorkflowPayload>,
    digest: &Sha256Digest,
    step_id: &str,
    direction: &str,
) -> Result<WorkflowDataSchema, String> {
    let payload = require_payload(payloads, digest, WorkflowPayloadKind::DataSchema, step_id)?;
    let WorkflowPayloadContent::DataSchema(schema) = payload.content() else {
        return Err(format!(
            "WorkflowRun step {step_id:?} {direction} schema content has the wrong kind"
        ));
    };
    Ok(schema.clone())
}

fn validate_branch_binding(
    step: &ResolvedWorkflowRunStep,
    edges: &[WorkflowEdgeSpec],
) -> Result<(), String> {
    if step.plan.kind != WorkflowStepKind::Branch {
        return Ok(());
    }
    let configured = step
        .configuration
        .routes
        .iter()
        .map(|route| route.handle.as_str())
        .collect::<BTreeSet<_>>();
    let outgoing = edges
        .iter()
        .filter(|edge| edge.source == step.plan.id)
        .filter_map(|edge| edge.source_handle.as_deref())
        .collect::<BTreeSet<_>>();
    if configured != outgoing {
        return Err(format!(
            "WorkflowRun branch {:?} route handles drifted from its plan edges",
            step.plan.id
        ));
    }
    Ok(())
}

pub fn workflow_run_timeout_seconds(value: Option<u64>) -> Result<u64, String> {
    let value = value.unwrap_or(WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS);
    if value == 0 || value > WORKFLOW_RUN_MAX_TIMEOUT_SECONDS {
        Err(format!(
            "WorkflowRun timeout must be between 1 and {WORKFLOW_RUN_MAX_TIMEOUT_SECONDS} seconds"
        ))
    } else {
        Ok(value)
    }
}
