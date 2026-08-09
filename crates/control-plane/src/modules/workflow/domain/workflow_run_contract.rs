use super::entities::digest_payload_set;
use super::{
    WorkflowDataSchema, WorkflowEdgeSpec, WorkflowPayload, WorkflowPayloadContent,
    WorkflowPayloadKind, WorkflowPlan, WorkflowPlanStep, WorkflowPolicy, WorkflowPolicyMode,
    WorkflowStepConfiguration, WorkflowStepKind, WORKFLOW_GOAL_MAX_INPUT_BYTES,
    WORKFLOW_PLAN_MAX_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, OrganizationId, PlanRevisionId, ProjectId, Sha256Digest,
    WorkflowGoalId, WorkflowRunId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_RUN_INPUT_SCHEMA: &str = "cloud.workflow-run.input.v1";
pub const WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION: &str = "cloud.workflow-run-runtime.v1";
pub const WORKFLOW_RUN_FLOW_NAME: &str = "cloud.workflow-run";
pub const WORKFLOW_RUN_FLOW_VERSION: &str = "1";
pub const WORKFLOW_RUN_INPUT_MAX_BYTES: usize = 8 * 1024 * 1024;
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
        canonical_json_bounded(self, WORKFLOW_RUN_INPUT_MAX_BYTES, "WorkflowRun input")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_RUN_INPUT_SCHEMA
            || self.runtime_contract_revision != WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION
            || self.flow_workflow_name != WORKFLOW_RUN_FLOW_NAME
            || self.flow_workflow_version != WORKFLOW_RUN_FLOW_VERSION
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
        for step in &resolved {
            if !matches!(
                step.plan.kind,
                WorkflowStepKind::Input
                    | WorkflowStepKind::Transform
                    | WorkflowStepKind::Branch
                    | WorkflowStepKind::Output
            ) {
                return Err(format!(
                    "WorkflowRun Phase 2 does not execute {} step {:?}",
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
                    "WorkflowRun Phase 2 rejects recorded-choice policy on step {:?}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest};
    use crate::modules::workflow::test_support::workflow_run_input;

    #[test]
    fn immutable_run_input_rejects_plan_input_payload_and_branch_drift() {
        let input = workflow_run_input().expect("valid WorkflowRun input");
        input.validate().expect("valid input");

        let mut goal_drift = input.clone();
        goal_drift.goal_input["priority"] = serde_json::json!("normal");
        assert!(goal_drift.validate().is_err());

        let mut payload_order_drift = input.clone();
        payload_order_drift.payloads.swap(0, 1);
        assert!(payload_order_drift.validate().is_err());

        let mut branch_drift = input;
        branch_drift
            .plan
            .edges
            .iter_mut()
            .find(|edge| edge.id == "route-high")
            .expect("high branch edge")
            .source_handle = Some("unexpected".into());
        branch_drift.plan_digest = Sha256Digest::parse(sha256_digest(
            &canonical_json_bounded(
                &branch_drift.plan,
                WORKFLOW_PLAN_MAX_BYTES,
                "WorkflowRun test plan",
            )
            .expect("canonical plan"),
        ))
        .expect("plan digest");
        assert!(branch_drift.validate().is_err());
    }

    #[test]
    fn workflow_run_timeout_is_strictly_bounded() {
        assert_eq!(
            workflow_run_timeout_seconds(None).expect("default timeout"),
            WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS
        );
        assert_eq!(workflow_run_timeout_seconds(Some(1)).expect("minimum"), 1);
        assert_eq!(
            workflow_run_timeout_seconds(Some(WORKFLOW_RUN_MAX_TIMEOUT_SECONDS)).expect("maximum"),
            WORKFLOW_RUN_MAX_TIMEOUT_SECONDS
        );
        assert!(workflow_run_timeout_seconds(Some(0)).is_err());
        assert!(workflow_run_timeout_seconds(Some(WORKFLOW_RUN_MAX_TIMEOUT_SECONDS + 1)).is_err());
    }
}
