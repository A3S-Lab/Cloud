use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, EnvironmentId, OntologyId,
    OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{
    has_application_answer_failure_route, has_application_variable_failure_route,
    has_connector_failure_route, has_transform_failure_route, has_workflow_output_failure_route,
    validate_descriptor_failure_routes, CapabilityReference, WorkflowContractQuotas,
    WorkflowEdgeSpec, WorkflowSpec, WorkflowStepDescriptorBinding, WorkflowStepFailureContract,
    WorkflowStepFallbackMode, WorkflowStepKind, WorkflowStepPort, WorkflowStepPortCardinality,
    WorkflowStepSpec,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_PLAN_SCHEMA: &str = "cloud.workflow.plan.v1";
pub const WORKFLOW_PLAN_COMPILER_REVISION: &str = "cloud.workflow.plan-compiler.v1";
pub const WORKFLOW_PLAN_SCHEMA_V2: &str = "cloud.workflow.plan.v2";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V2: &str = "cloud.workflow.plan-compiler.v2";
pub const WORKFLOW_PLAN_SCHEMA_V3: &str = "cloud.workflow.plan.v3";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V3: &str = "cloud.workflow.plan-compiler.v3";
pub const WORKFLOW_PLAN_SCHEMA_V4: &str = "cloud.workflow.plan.v4";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V4: &str = "cloud.workflow.plan-compiler.v4";
pub const WORKFLOW_PLAN_SCHEMA_V5: &str = "cloud.workflow.plan.v5";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V5: &str = "cloud.workflow.plan-compiler.v5";
pub const WORKFLOW_PLAN_SCHEMA_V6: &str = "cloud.workflow.plan.v6";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V6: &str = "cloud.workflow.plan-compiler.v6";
pub const WORKFLOW_PLAN_SCHEMA_V7: &str = "cloud.workflow.plan.v7";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V7: &str = "cloud.workflow.plan-compiler.v7";
pub const WORKFLOW_PLAN_SCHEMA_V8: &str = "cloud.workflow.plan.v8";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V8: &str = "cloud.workflow.plan-compiler.v8";
pub const WORKFLOW_PLAN_SCHEMA_V9: &str = "cloud.workflow.plan.v9";
pub const WORKFLOW_PLAN_COMPILER_REVISION_V9: &str = "cloud.workflow.plan-compiler.v9";
pub const WORKFLOW_PLAN_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowStepDefaultOutputContract {
    pub output_port: WorkflowStepPort,
}

impl WorkflowStepDefaultOutputContract {
    pub fn validate(&self) -> Result<(), String> {
        super::super::validation::validate_identifier(
            "Workflow default-output port",
            &self.output_port.name,
        )?;
        if self.output_port.cardinality != WorkflowStepPortCardinality::Single
            || !self.output_port.required
            || self.output_port.dynamic
        {
            return Err(
                "Workflow default output must use one required static descriptor port".into(),
            );
        }
        Ok(())
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<WorkflowStepDescriptorBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<WorkflowStepFailureContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_output: Option<WorkflowStepDefaultOutputContract>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_contract_set_digest: Option<Sha256Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_contract_digest: Option<Sha256Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite_regions_digest: Option<Sha256Digest>,
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
        let version = match (self.schema.as_str(), self.compiler_revision.as_str()) {
            (WORKFLOW_PLAN_SCHEMA, WORKFLOW_PLAN_COMPILER_REVISION) => WorkflowPlanVersion::V1,
            (WORKFLOW_PLAN_SCHEMA_V2, WORKFLOW_PLAN_COMPILER_REVISION_V2) => {
                WorkflowPlanVersion::V2
            }
            (WORKFLOW_PLAN_SCHEMA_V3, WORKFLOW_PLAN_COMPILER_REVISION_V3) => {
                WorkflowPlanVersion::V3
            }
            (WORKFLOW_PLAN_SCHEMA_V4, WORKFLOW_PLAN_COMPILER_REVISION_V4) => {
                WorkflowPlanVersion::V4
            }
            (WORKFLOW_PLAN_SCHEMA_V5, WORKFLOW_PLAN_COMPILER_REVISION_V5) => {
                WorkflowPlanVersion::V5
            }
            (WORKFLOW_PLAN_SCHEMA_V6, WORKFLOW_PLAN_COMPILER_REVISION_V6) => {
                WorkflowPlanVersion::V6
            }
            (WORKFLOW_PLAN_SCHEMA_V7, WORKFLOW_PLAN_COMPILER_REVISION_V7) => {
                WorkflowPlanVersion::V7
            }
            (WORKFLOW_PLAN_SCHEMA_V8, WORKFLOW_PLAN_COMPILER_REVISION_V8) => {
                WorkflowPlanVersion::V8
            }
            (WORKFLOW_PLAN_SCHEMA_V9, WORKFLOW_PLAN_COMPILER_REVISION_V9) => {
                WorkflowPlanVersion::V9
            }
            _ => return Err("Workflow plan schema and compiler revision are incompatible".into()),
        };
        let semantic_version = version != WorkflowPlanVersion::V1;
        let failure_version = matches!(
            version,
            WorkflowPlanVersion::V3
                | WorkflowPlanVersion::V4
                | WorkflowPlanVersion::V5
                | WorkflowPlanVersion::V6
                | WorkflowPlanVersion::V7
                | WorkflowPlanVersion::V8
                | WorkflowPlanVersion::V9
        );
        let default_output_capable = matches!(
            version,
            WorkflowPlanVersion::V4
                | WorkflowPlanVersion::V5
                | WorkflowPlanVersion::V6
                | WorkflowPlanVersion::V7
                | WorkflowPlanVersion::V8
                | WorkflowPlanVersion::V9
        );
        if self.workflow_definition_id.as_uuid().is_nil()
            || self.workflow_revision_id.as_uuid().is_nil()
            || self.ontology_id.as_uuid().is_nil()
            || self.ontology_revision_id.as_uuid().is_nil()
            || self
                .environment_id
                .is_some_and(|environment_id| environment_id.as_uuid().is_nil())
        {
            return Err("Workflow plan authority bindings are invalid".into());
        }
        match (
            semantic_version,
            self.semantic_contract_set_digest.as_ref(),
            self.variable_contract_digest.as_ref(),
            self.composite_regions_digest.as_ref(),
        ) {
            (false, None, None, None) | (true, Some(_), Some(_), _) => {}
            _ => return Err("Workflow plan semantic contract bindings are invalid".into()),
        }
        for step in self
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Service && step.capability.is_none())
        {
            if !semantic_version {
                return Err(
                    "Legacy Workflow plans cannot contain a capability-free Service step".into(),
                );
            }
            if step.descriptor.as_ref().is_none_or(|descriptor| {
                descriptor.descriptor_id != "application.conversation-variable-assign"
            }) {
                return Err(
                    "Capability-free Workflow Service plans require the exact Application variable descriptor"
                        .into(),
                );
            }
        }
        let workflow = self.workflow_spec()?;
        let order = workflow.topological_order(WorkflowContractQuotas::default())?;
        if self
            .steps
            .iter()
            .any(|step| step.descriptor.is_some() != semantic_version)
        {
            return Err("Workflow plan step descriptor bindings are incomplete".into());
        }
        if semantic_version
            && self.steps.iter().any(|step| {
                step.descriptor
                    .as_ref()
                    .is_none_or(|descriptor| descriptor.step_id != step.id)
            })
        {
            return Err("Workflow plan descriptor binding targets the wrong step".into());
        }
        if self
            .steps
            .iter()
            .any(|step| step.failure.is_some() != failure_version)
        {
            return Err("Workflow plan step failure contracts are incomplete".into());
        }
        if !default_output_capable && self.steps.iter().any(|step| step.default_output.is_some()) {
            return Err("Workflow plan default-output contracts require Plan v4".into());
        }
        let failures = self
            .steps
            .iter()
            .filter_map(|step| {
                step.failure
                    .as_ref()
                    .map(|failure| (step.id.as_str(), failure))
            })
            .collect::<BTreeMap<_, _>>();
        let application_variable_steps = self
            .steps
            .iter()
            .filter(|step| {
                step.kind == WorkflowStepKind::Service
                    && step.capability.is_none()
                    && step.descriptor.as_ref().is_some_and(|descriptor| {
                        descriptor.descriptor_id == "application.conversation-variable-assign"
                    })
            })
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        let application_answer_steps =
            self.steps
                .iter()
                .filter(|step| {
                    step.kind == WorkflowStepKind::Output
                        && step.capability.is_none()
                        && step.descriptor.as_ref().is_some_and(|descriptor| {
                            descriptor.descriptor_id == "application.answer"
                        })
                })
                .map(|step| step.id.as_str())
                .collect::<BTreeSet<_>>();
        let workflow_output_steps = self
            .steps
            .iter()
            .filter(|step| {
                step.kind == WorkflowStepKind::Output
                    && step.capability.is_none()
                    && step
                        .descriptor
                        .as_ref()
                        .is_some_and(|descriptor| descriptor.descriptor_id == "workflow.output")
            })
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        let has_failure_routes = if failure_version {
            validate_descriptor_failure_routes(
                &workflow,
                &failures,
                &application_variable_steps,
                &application_answer_steps,
                &workflow_output_steps,
            )?
        } else {
            workflow.has_non_branch_source_handles()
        };
        let has_connector_failure_routes = has_connector_failure_route(&workflow);
        let has_application_variable_failure_routes =
            has_application_variable_failure_route(&workflow, &application_variable_steps);
        let has_application_answer_failure_routes =
            has_application_answer_failure_route(&workflow, &application_answer_steps);
        let has_transform_failure_routes = has_transform_failure_route(&workflow);
        let has_workflow_output_failure_routes =
            has_workflow_output_failure_route(&workflow, &workflow_output_steps);
        let has_default_outputs = validate_default_output_contracts(self)?;
        match version {
            WorkflowPlanVersion::V1 | WorkflowPlanVersion::V2
                if has_failure_routes || has_default_outputs =>
            {
                return Err("Workflow plan failure semantics require a newer version".into())
            }
            WorkflowPlanVersion::V3
                if !has_failure_routes
                    || has_connector_failure_routes
                    || has_application_variable_failure_routes
                    || has_application_answer_failure_routes
                    || has_transform_failure_routes
                    || has_workflow_output_failure_routes
                    || has_default_outputs =>
            {
                return Err(
                    "Workflow Plan v3 must contain only finite-Execution routed failure semantics"
                        .into(),
                )
            }
            WorkflowPlanVersion::V4 if !has_default_outputs => {
                return Err("Workflow Plan v4 requires at least one default-output fallback".into())
            }
            WorkflowPlanVersion::V4
                if has_connector_failure_routes
                    || has_application_variable_failure_routes
                    || has_application_answer_failure_routes
                    || has_transform_failure_routes
                    || has_workflow_output_failure_routes =>
            {
                return Err(
                    "Workflow Plan v4 cannot contain a Connector, Application, Transform, or Output failure route"
                        .into(),
                )
            }
            WorkflowPlanVersion::V5
                if !has_connector_failure_routes
                    || has_application_variable_failure_routes
                    || has_application_answer_failure_routes
                    || has_transform_failure_routes
                    || has_workflow_output_failure_routes =>
            {
                return Err(
                    "Workflow Plan v5 requires Connector failure routes without Application, Transform, or Output failure routes"
                        .into(),
                )
            }
            WorkflowPlanVersion::V6 if !has_application_variable_failure_routes => {
                return Err(
                    "Workflow Plan v6 requires at least one descriptor-bound Application variable failure route"
                        .into(),
                )
            }
            WorkflowPlanVersion::V6
                if has_application_answer_failure_routes
                    || has_transform_failure_routes
                    || has_workflow_output_failure_routes =>
            {
                return Err(
                    "Workflow Plan v6 cannot contain an Application Answer, Transform, or Output failure route"
                        .into(),
                )
            }
            WorkflowPlanVersion::V7 if !has_application_answer_failure_routes => {
                return Err(
                    "Workflow Plan v7 requires at least one descriptor-bound Application Answer failure route"
                    .into(),
                )
            }
            WorkflowPlanVersion::V7
                if has_transform_failure_routes || has_workflow_output_failure_routes =>
            {
                return Err(
                    "Workflow Plan v7 cannot contain a Workflow-local failure route".into(),
                )
            }
            WorkflowPlanVersion::V8 if !has_transform_failure_routes => {
                return Err(
                    "Workflow Plan v8 requires at least one descriptor-bound Transform failure route"
                        .into(),
                )
            }
            WorkflowPlanVersion::V8 if has_workflow_output_failure_routes => {
                return Err("Workflow Plan v8 cannot contain an Output failure route".into())
            }
            WorkflowPlanVersion::V9 if !has_workflow_output_failure_routes => {
                return Err(
                    "Workflow Plan v9 requires at least one descriptor-bound Output failure route"
                        .into(),
                )
            }
            _ => {}
        }
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

    pub(crate) fn workflow_spec(&self) -> Result<WorkflowSpec, String> {
        let mut ids = BTreeSet::new();
        Ok(WorkflowSpec {
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
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowPlanVersion {
    V1,
    V2,
    V3,
    V4,
    V5,
    V6,
    V7,
    V8,
    V9,
}

fn validate_default_output_contracts(plan: &WorkflowPlan) -> Result<bool, String> {
    let handled_sources = plan
        .edges
        .iter()
        .filter_map(|edge| edge.source_handle.as_ref().map(|_| edge.source.as_str()))
        .collect::<BTreeSet<_>>();
    let mut found = false;
    for step in &plan.steps {
        let fallback = step.failure.as_ref().map(|failure| failure.fallback);
        match (fallback, step.default_output.as_ref()) {
            (Some(WorkflowStepFallbackMode::DefaultOutput), Some(contract)) => {
                found = true;
                contract.validate()?;
                if step.kind != WorkflowStepKind::Execution
                    || step.policy_digest.is_none()
                    || handled_sources.contains(step.id.as_str())
                {
                    return Err(format!(
                        "Workflow step {:?} has an invalid default-output fallback binding",
                        step.id
                    ));
                }
            }
            (Some(WorkflowStepFallbackMode::DefaultOutput), None) => {
                return Err(format!(
                    "Workflow step {:?} lost its default-output contract",
                    step.id
                ))
            }
            (_, Some(_)) => {
                return Err(format!(
                    "Workflow step {:?} has default-output material without descriptor fallback",
                    step.id
                ))
            }
            _ => {}
        }
    }
    Ok(found)
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
