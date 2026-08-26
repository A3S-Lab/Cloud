use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{
    validate_list_operator_binding, validate_variable_aggregate_binding, CapabilityType,
    WorkflowContract, WorkflowEdgeSpec, WorkflowPayload, WorkflowPayloadContent,
    WorkflowPayloadKind, WorkflowPolicy, WorkflowRevisionSemanticContracts, WorkflowSpec,
    WorkflowStepFailureContract, WorkflowStepKind, WorkflowStepSpec, WORKFLOW_DEFINITION_SCHEMA,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_COMPILER_SCHEMA_VERSION: u32 = 1;
pub const WORKFLOW_COMPILER_SCHEMA_VERSION_V2: u32 = 2;
pub const WORKFLOW_REVISION_MAX_PAYLOADS: usize = 2_048;
pub const WORKFLOW_REVISION_MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub id: WorkflowRevisionId,
    pub revision_number: u64,
    pub parent_revision_id: Option<WorkflowRevisionId>,
    pub parent_digest: Option<Sha256Digest>,
    pub contract: WorkflowContract,
    pub payloads: Vec<WorkflowPayload>,
    pub payload_set_digest: Sha256Digest,
    pub semantic_contracts: Option<WorkflowRevisionSemanticContracts>,
    pub compiler_schema_version: u32,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl WorkflowRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn initial(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
        id: WorkflowRevisionId,
        contract: WorkflowContract,
        payloads: Vec<WorkflowPayload>,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::build(
            organization_id,
            project_id,
            workflow_definition_id,
            id,
            1,
            None,
            None,
            contract,
            payloads,
            None,
            WORKFLOW_COMPILER_SCHEMA_VERSION,
            created_by,
            created_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn initial_with_semantic_contracts(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
        id: WorkflowRevisionId,
        contract: WorkflowContract,
        payloads: Vec<WorkflowPayload>,
        semantic_contracts: WorkflowRevisionSemanticContracts,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::build(
            organization_id,
            project_id,
            workflow_definition_id,
            id,
            1,
            None,
            None,
            contract,
            payloads,
            Some(semantic_contracts),
            WORKFLOW_COMPILER_SCHEMA_VERSION_V2,
            created_by,
            created_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn successor(
        parent: &Self,
        id: WorkflowRevisionId,
        contract: WorkflowContract,
        payloads: Vec<WorkflowPayload>,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if parent.compiler_schema_version != WORKFLOW_COMPILER_SCHEMA_VERSION {
            return Err(
                "Workflow revisions cannot remove an established semantic contract authority"
                    .into(),
            );
        }
        let revision_number = parent
            .revision_number
            .checked_add(1)
            .ok_or_else(|| "Workflow revision number is exhausted".to_owned())?;
        Self::build(
            parent.organization_id,
            parent.project_id,
            parent.workflow_definition_id,
            id,
            revision_number,
            Some(parent.id),
            Some(parent.contract.digest().clone()),
            contract,
            payloads,
            None,
            WORKFLOW_COMPILER_SCHEMA_VERSION,
            created_by,
            created_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn successor_with_semantic_contracts(
        parent: &Self,
        id: WorkflowRevisionId,
        contract: WorkflowContract,
        payloads: Vec<WorkflowPayload>,
        semantic_contracts: WorkflowRevisionSemanticContracts,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let revision_number = parent
            .revision_number
            .checked_add(1)
            .ok_or_else(|| "Workflow revision number is exhausted".to_owned())?;
        Self::build(
            parent.organization_id,
            parent.project_id,
            parent.workflow_definition_id,
            id,
            revision_number,
            Some(parent.id),
            Some(parent.contract.digest().clone()),
            contract,
            payloads,
            Some(semantic_contracts),
            WORKFLOW_COMPILER_SCHEMA_VERSION_V2,
            created_by,
            created_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
        id: WorkflowRevisionId,
        revision_number: u64,
        parent_revision_id: Option<WorkflowRevisionId>,
        parent_digest: Option<Sha256Digest>,
        acl: &str,
        stored_digest: &str,
        payloads: Vec<WorkflowPayload>,
        stored_payload_set_digest: &str,
        semantic_contracts: Option<WorkflowRevisionSemanticContracts>,
        compiler_schema_version: u32,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self::build(
            organization_id,
            project_id,
            workflow_definition_id,
            id,
            revision_number,
            parent_revision_id,
            parent_digest,
            WorkflowContract::restore(acl, stored_digest)?,
            payloads,
            semantic_contracts,
            compiler_schema_version,
            created_by,
            created_at,
        )?;
        if value.payload_set_digest.as_str() != stored_payload_set_digest {
            return Err("stored Workflow payload-set digest does not match its payloads".into());
        }
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
        id: WorkflowRevisionId,
        revision_number: u64,
        parent_revision_id: Option<WorkflowRevisionId>,
        parent_digest: Option<Sha256Digest>,
        contract: WorkflowContract,
        mut payloads: Vec<WorkflowPayload>,
        semantic_contracts: Option<WorkflowRevisionSemanticContracts>,
        compiler_schema_version: u32,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
        let payload_set_digest = digest_payload_set(&payloads)?;
        let value = Self {
            organization_id,
            project_id,
            workflow_definition_id,
            id,
            revision_number,
            parent_revision_id,
            parent_digest,
            contract,
            payloads,
            payload_set_digest,
            semantic_contracts,
            compiler_schema_version,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.workflow_definition_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.revision_number == 0
        {
            return Err("stored Workflow revision is invalid".into());
        }
        match (&self.semantic_contracts, self.compiler_schema_version) {
            (None, WORKFLOW_COMPILER_SCHEMA_VERSION)
            | (Some(_), WORKFLOW_COMPILER_SCHEMA_VERSION_V2) => {}
            _ => return Err("Workflow revision semantic contract version is invalid".into()),
        }
        match (&self.parent_revision_id, &self.parent_digest) {
            (None, None) if self.revision_number == 1 => {}
            (Some(parent_id), Some(_))
                if self.revision_number > 1 && !parent_id.as_uuid().is_nil() => {}
            _ => return Err("Workflow revision lineage is invalid".into()),
        }
        if let Some(contracts) = &self.semantic_contracts {
            contracts.validate(self.contract.spec())?;
        } else {
            if self
                .contract
                .spec()
                .steps
                .iter()
                .any(|step| step.kind == WorkflowStepKind::Service && step.capability.is_none())
            {
                return Err(
                    "Workflow Service steps without ConnectorRevision require immutable descriptor semantic contracts"
                        .into(),
                );
            }
            if self.contract.spec().has_non_branch_source_handles() {
                return Err(
                    "Workflow failure routes require immutable descriptor semantic contracts"
                        .into(),
                );
            }
        }
        validate_payload_bindings(
            &self.contract,
            &self.payloads,
            self.semantic_contracts.as_ref(),
        )?;
        if digest_payload_set(&self.payloads)? != self.payload_set_digest {
            return Err("Workflow revision payload-set digest is invalid".into());
        }
        Ok(())
    }

    pub const fn contract_schema(&self) -> &'static str {
        WORKFLOW_DEFINITION_SCHEMA
    }

    pub fn payload(&self, digest: &Sha256Digest) -> Option<&WorkflowPayload> {
        self.payloads
            .binary_search_by(|payload| payload.digest().cmp(digest))
            .ok()
            .map(|index| &self.payloads[index])
    }

    pub fn semantic_contract_set_digest(&self) -> Option<&Sha256Digest> {
        self.semantic_contracts
            .as_ref()
            .map(WorkflowRevisionSemanticContracts::digest)
    }

    pub(crate) fn validate_runtime_dispatch_support(&self) -> Result<(), String> {
        validate_runtime_dispatch_support(self.contract.spec(), self.semantic_contracts.as_ref())
    }

    pub(crate) fn has_variable_aggregate_configuration(&self) -> bool {
        self.payloads.iter().any(|payload| {
            matches!(
                payload.content(),
                WorkflowPayloadContent::Configuration(configuration)
                    if configuration.variable_aggregate().is_some()
            )
        })
    }

    pub(crate) fn has_list_operator_configuration(&self) -> bool {
        self.payloads.iter().any(|payload| {
            matches!(
                payload.content(),
                WorkflowPayloadContent::Configuration(configuration)
                    if configuration.list_operator().is_some()
            )
        })
    }

    pub(crate) fn has_cancellation_compensation(&self) -> bool {
        self.payloads.iter().any(|payload| {
            matches!(
                payload.content(),
                WorkflowPayloadContent::Policy(policy)
                    if policy.cancellation_compensation.is_some()
            )
        })
    }

    pub(crate) fn has_agent_step(&self) -> bool {
        self.contract
            .spec()
            .steps
            .iter()
            .any(|step| step.kind == super::super::WorkflowStepKind::Agent)
    }
}

fn validate_runtime_dispatch_support(
    workflow: &WorkflowSpec,
    semantic_contracts: Option<&WorkflowRevisionSemanticContracts>,
) -> Result<(), String> {
    if let Some(contracts) = semantic_contracts {
        return contracts.validate_runtime_dispatch_support(workflow);
    }
    for step in &workflow.steps {
        if matches!(
            step.kind,
            WorkflowStepKind::Input
                | WorkflowStepKind::Output
                | WorkflowStepKind::Transform
                | WorkflowStepKind::Branch
                | WorkflowStepKind::HumanDecision
                | WorkflowStepKind::Execution
                | WorkflowStepKind::Service
        ) {
            continue;
        }
        return Err(format!(
            "Workflow step {:?} kind {:?} has no admitted Cloud runtime dispatch port without immutable descriptor semantic contracts",
            step.id,
            step.kind.as_str()
        ));
    }
    Ok(())
}

fn validate_payload_bindings(
    contract: &WorkflowContract,
    payloads: &[WorkflowPayload],
    semantic_contracts: Option<&WorkflowRevisionSemanticContracts>,
) -> Result<(), String> {
    if payloads.is_empty() || payloads.len() > WORKFLOW_REVISION_MAX_PAYLOADS {
        return Err(format!(
            "Workflow revision must contain between 1 and {WORKFLOW_REVISION_MAX_PAYLOADS} payloads"
        ));
    }
    let total_bytes = payloads.iter().try_fold(0usize, |total, payload| {
        total
            .checked_add(payload.canonical_acl().len())
            .ok_or_else(|| "Workflow payload byte total overflowed".to_owned())
    })?;
    if total_bytes > WORKFLOW_REVISION_MAX_PAYLOAD_BYTES {
        return Err(format!(
            "Workflow revision payloads exceed {WORKFLOW_REVISION_MAX_PAYLOAD_BYTES} bytes"
        ));
    }
    let by_digest = payloads
        .iter()
        .map(|payload| (payload.digest(), payload))
        .collect::<BTreeMap<_, _>>();
    if by_digest.len() != payloads.len() {
        return Err("Workflow revision contains duplicate payload digests".into());
    }

    let mut referenced = BTreeSet::new();
    let mut policies = BTreeMap::<&str, &WorkflowPolicy>::new();
    for step in &contract.spec().steps {
        let configuration = require_payload(
            &by_digest,
            &step.configuration_digest,
            WorkflowPayloadKind::Configuration,
            &step.id,
        )?;
        let WorkflowPayloadContent::Configuration(configuration) = configuration.content() else {
            return Err("Workflow configuration payload content has the wrong kind".into());
        };
        if configuration.step_kind != step.kind {
            return Err(format!(
                "Workflow step {:?} configuration targets {}, not {}",
                step.id,
                configuration.step_kind.as_str(),
                step.kind.as_str()
            ));
        }
        referenced.insert(step.configuration_digest.clone());
        let input_payload = require_payload(
            &by_digest,
            &step.input_schema_digest,
            WorkflowPayloadKind::DataSchema,
            &step.id,
        )?;
        let WorkflowPayloadContent::DataSchema(input_schema) = input_payload.content() else {
            return Err("Workflow input schema payload content has the wrong kind".into());
        };
        referenced.insert(step.input_schema_digest.clone());
        let output_payload = require_payload(
            &by_digest,
            &step.output_schema_digest,
            WorkflowPayloadKind::DataSchema,
            &step.id,
        )?;
        let WorkflowPayloadContent::DataSchema(output_schema) = output_payload.content() else {
            return Err("Workflow output schema payload content has the wrong kind".into());
        };
        referenced.insert(step.output_schema_digest.clone());
        let policy = step
            .policy_digest
            .as_ref()
            .map(|digest| {
                let payload =
                    require_payload(&by_digest, digest, WorkflowPayloadKind::Policy, &step.id)?;
                let WorkflowPayloadContent::Policy(policy) = payload.content() else {
                    return Err("Workflow policy payload content has the wrong kind".into());
                };
                referenced.insert(digest.clone());
                Ok::<&WorkflowPolicy, String>(policy)
            })
            .transpose()?;
        validate_retry_policy_binding(step, policy)?;
        validate_default_output_policy_binding(step, policy, output_schema, semantic_contracts)?;
        if let Some(policy) = policy {
            policies.insert(step.id.as_str(), policy);
        }
        validate_list_operator_binding(
            step,
            configuration,
            input_schema,
            output_schema,
            semantic_contracts,
        )?;
        validate_variable_aggregate_binding(
            step,
            configuration,
            input_schema,
            output_schema,
            semantic_contracts,
        )?;
        if step.kind == WorkflowStepKind::Branch {
            let failure = semantic_contracts
                .map(|contracts| contracts.failure_contract(&step.id))
                .transpose()?;
            validate_branch_handles(contract, &step.id, configuration, failure)?;
        }
    }
    validate_cancellation_compensation_bindings(contract.spec(), &policies)?;
    let stored = payloads
        .iter()
        .map(|payload| payload.digest().clone())
        .collect::<BTreeSet<_>>();
    if referenced != stored {
        return Err(
            "Workflow revision must store exactly the payloads referenced by its definition".into(),
        );
    }
    Ok(())
}

fn validate_cancellation_compensation_bindings(
    workflow: &WorkflowSpec,
    policies: &BTreeMap<&str, &WorkflowPolicy>,
) -> Result<(), String> {
    let steps = workflow
        .steps
        .iter()
        .map(|step| (step.id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    workflow.topological_order(Default::default())?;
    let mut targets = BTreeSet::new();
    for source in &workflow.steps {
        let Some(compensation) = policies
            .get(source.id.as_str())
            .and_then(|policy| policy.cancellation_compensation.as_ref())
        else {
            continue;
        };
        let Some(target) = steps.get(compensation.step_id.as_str()).copied() else {
            return Err(format!(
                "Workflow Connector step {:?} references missing cancellation compensation {:?}",
                source.id, compensation.step_id
            ));
        };
        if source.id == target.id {
            return Err(format!(
                "Workflow Connector step {:?} cannot compensate itself",
                source.id
            ));
        }
        if !is_exact_connector_step(source) || !is_exact_connector_step(target) {
            return Err(format!(
                "Workflow cancellation compensation {:?} -> {:?} requires exact connector.http Service steps",
                source.id, target.id
            ));
        }
        if source.output_schema_digest != target.input_schema_digest {
            return Err(format!(
                "Workflow cancellation compensation {:?} -> {:?} has incompatible output and input schemas",
                source.id, target.id
            ));
        }
        if !workflow_path_exists(&workflow.edges, &source.id, &target.id) {
            return Err(format!(
                "Workflow cancellation compensation {:?} must be downstream of {:?}",
                target.id, source.id
            ));
        }
        if policies
            .get(target.id.as_str())
            .is_some_and(|policy| policy.cancellation_compensation.is_some())
        {
            return Err(format!(
                "Workflow cancellation compensation target {:?} cannot own another compensation",
                target.id
            ));
        }
        if !targets.insert(target.id.as_str()) {
            return Err(format!(
                "Workflow cancellation compensation target {:?} is assigned more than once",
                target.id
            ));
        }
        let incoming = workflow
            .edges
            .iter()
            .filter(|edge| edge.target == target.id)
            .collect::<Vec<_>>();
        if incoming.len() != 1 || incoming[0].source_handle.is_none() {
            return Err(format!(
                "Workflow cancellation compensation target {:?} must also be reachable through one explicit handled route",
                target.id
            ));
        }
    }
    Ok(())
}

fn workflow_path_exists(edges: &[WorkflowEdgeSpec], source: &str, target: &str) -> bool {
    let mut pending = vec![source.to_owned()];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        for edge in edges.iter().filter(|edge| edge.source == current) {
            if edge.target == target {
                return true;
            }
            pending.push(edge.target.clone());
        }
    }
    false
}

fn is_exact_connector_step(step: &WorkflowStepSpec) -> bool {
    step.kind == WorkflowStepKind::Service
        && step.capability.as_ref().is_some_and(|capability| {
            capability.capability_type == CapabilityType::ConnectorRevision
                && capability.capability == "connector.http"
        })
}

fn validate_default_output_policy_binding(
    step: &WorkflowStepSpec,
    policy: Option<&WorkflowPolicy>,
    output_schema: &crate::modules::workflow::domain::WorkflowDataSchema,
    semantic_contracts: Option<&WorkflowRevisionSemanticContracts>,
) -> Result<(), String> {
    let expected = semantic_contracts
        .map(|contracts| contracts.default_output_contract(&step.id))
        .transpose()?
        .flatten();
    let material = policy.and_then(|policy| policy.default_output.as_ref());
    match (expected, material) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(format!(
            "Workflow step {:?} has default-output policy material without descriptor authority",
            step.id
        )),
        (Some(_), None) => Err(format!(
            "Workflow step {:?} requires exact default-output policy material",
            step.id
        )),
        (Some(expected), Some(material)) => {
            if material.port != expected.output_port.name {
                return Err(format!(
                    "Workflow step {:?} default-output port {:?} does not match descriptor port {:?}",
                    step.id, material.port, expected.output_port.name
                ));
            }
            if !expected
                .output_port
                .value_type
                .matches_json_value(&material.value)
            {
                return Err(format!(
                    "Workflow step {:?} default output does not match descriptor type {}",
                    step.id,
                    expected.output_port.value_type.as_str()
                ));
            }
            output_schema.validate_value(
                &material.value,
                &format!("Workflow step {:?} default output", step.id),
            )
        }
    }
}

fn validate_retry_policy_binding(
    step: &WorkflowStepSpec,
    policy: Option<&WorkflowPolicy>,
) -> Result<(), String> {
    let connector = step
        .capability
        .as_ref()
        .is_some_and(|capability| capability.capability_type == CapabilityType::ConnectorRevision);
    let retry = policy.and_then(|policy| policy.retry.as_ref());
    match (connector, retry) {
        (true, Some(_)) | (false, None) => Ok(()),
        (true, None) => Err(format!(
            "Workflow Connector step {:?} requires an exact retry budget",
            step.id
        )),
        (false, Some(_)) => Err(format!(
            "Workflow step {:?} cannot use provider retry policy before its owning runtime is admitted",
            step.id
        )),
    }
}

fn require_payload<'a>(
    by_digest: &'a BTreeMap<&Sha256Digest, &'a WorkflowPayload>,
    digest: &Sha256Digest,
    kind: WorkflowPayloadKind,
    step_id: &str,
) -> Result<&'a WorkflowPayload, String> {
    let payload = by_digest.get(digest).copied().ok_or_else(|| {
        format!(
            "Workflow step {step_id:?} references missing {} payload {digest}",
            kind.as_str()
        )
    })?;
    if payload.kind() != kind {
        return Err(format!(
            "Workflow step {step_id:?} references {digest} as {}, but it is {}",
            kind.as_str(),
            payload.kind().as_str()
        ));
    }
    Ok(payload)
}

fn validate_branch_handles(
    contract: &WorkflowContract,
    step_id: &str,
    configuration: &crate::modules::workflow::domain::WorkflowStepConfiguration,
    failure: Option<&WorkflowStepFailureContract>,
) -> Result<(), String> {
    let configured = configuration
        .routes
        .iter()
        .map(|route| route.handle.as_str())
        .collect::<BTreeSet<_>>();
    let default = configuration
        .default_handle
        .as_deref()
        .ok_or_else(|| "Workflow branch default handle is missing".to_owned())?;
    if !configured.contains(default) {
        return Err(format!(
            "Workflow branch {step_id:?} default handle is not a declared route"
        ));
    }
    let failure_handle = failure
        .and_then(|contract| contract.error_output.as_ref())
        .map(|output| output.name.as_str());
    if failure_handle.is_some_and(|handle| configured.contains(handle)) {
        return Err(format!(
            "Workflow branch {step_id:?} descriptor error handle conflicts with a business route"
        ));
    }
    let outgoing = contract
        .spec()
        .edges
        .iter()
        .filter(|edge| edge.source == step_id)
        .filter_map(|edge| edge.source_handle.as_deref())
        .collect::<BTreeSet<_>>();
    let ordinary_outgoing = outgoing
        .iter()
        .copied()
        .filter(|handle| Some(*handle) != failure_handle)
        .collect::<BTreeSet<_>>();
    if configured != ordinary_outgoing {
        return Err(format!(
            "Workflow branch {step_id:?} routes do not exactly match its ordinary outgoing handles"
        ));
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PayloadDigestEntry<'a> {
    kind: &'a str,
    schema: &'a str,
    digest: &'a str,
}

pub(crate) fn digest_payload_set(payloads: &[WorkflowPayload]) -> Result<Sha256Digest, String> {
    let entries = payloads
        .iter()
        .map(|payload| PayloadDigestEntry {
            kind: payload.kind().as_str(),
            schema: payload.schema(),
            digest: payload.digest().as_str(),
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_vec(&entries)
        .map_err(|error| format!("could not encode Workflow payload set: {error}"))?;
    Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(encoded)))
}

#[cfg(test)]
mod runtime_dispatch_tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        OntologyId, OntologyRevisionId, PlanRevisionId, WorkflowGoalId, WorkflowRunId,
    };
    use crate::modules::workflow::domain::{
        CapabilityReference, OntologyContract, OntologyObjectType, OntologyRevision, OntologySpec,
        PlanRevision, WorkflowDataSchema, WorkflowDataType, WorkflowDefinition, WorkflowEdgeSpec,
        WorkflowGoal, WorkflowGoalContract, WorkflowGoalSpec, WorkflowPayloadContent, WorkflowPlan,
        WorkflowPlanCompiler, WorkflowPlanStep, WorkflowRunCompiler, WorkflowSpec,
        WorkflowStepConfiguration, WORKFLOW_PLAN_COMPILER_REVISION, WORKFLOW_PLAN_SCHEMA,
    };
    use serde_json::json;
    use uuid::Uuid;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn workflow(kind: WorkflowStepKind) -> WorkflowSpec {
        let step = |id: &str, kind: WorkflowStepKind| WorkflowStepSpec {
            id: id.into(),
            label: id.into(),
            kind,
            configuration_digest: digest('a'),
            input_schema_digest: digest('b'),
            output_schema_digest: digest('c'),
            policy_digest: None,
            capability: None,
        };
        WorkflowSpec {
            name: "Runtime dispatch".into(),
            description: String::new(),
            steps: vec![
                step("input", WorkflowStepKind::Input),
                step("target", kind),
                step("output", WorkflowStepKind::Output),
            ],
            edges: Vec::new(),
        }
    }

    #[test]
    fn semantic_free_runtime_admission_is_closed_over_the_wired_dispatch_set() {
        for supported in [
            WorkflowStepKind::Input,
            WorkflowStepKind::Output,
            WorkflowStepKind::Transform,
            WorkflowStepKind::Branch,
            WorkflowStepKind::HumanDecision,
            WorkflowStepKind::Execution,
            WorkflowStepKind::Service,
        ] {
            validate_runtime_dispatch_support(&workflow(supported), None)
                .expect("wired semantic-free dispatch");
        }
        for unsupported in [
            WorkflowStepKind::Agent,
            WorkflowStepKind::Mcp,
            WorkflowStepKind::Model,
            WorkflowStepKind::Tool,
            WorkflowStepKind::Memory,
            WorkflowStepKind::Subworkflow,
        ] {
            let error = validate_runtime_dispatch_support(&workflow(unsupported), None)
                .expect_err("unwired semantic-free dispatch must fail closed");
            assert!(
                error.contains("has no admitted Cloud runtime dispatch port"),
                "unexpected {unsupported:?} admission error: {error}"
            );
        }
    }

    #[test]
    fn new_plan_and_run_compilation_reject_historic_unwired_revisions() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let definition_id = WorkflowDefinitionId::new();
        let revision_id = WorkflowRevisionId::new();
        let principal_id = PrincipalId::new();
        let now = Utc::now();

        let data_schema =
            WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
                value_type: WorkflowDataType::Object,
                fields: Vec::new(),
            }))
            .expect("data schema");
        let configuration = |kind| {
            WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
                WorkflowStepConfiguration::empty(kind),
            ))
            .expect("step configuration")
        };
        let input_configuration = configuration(WorkflowStepKind::Input);
        let agent_configuration = configuration(WorkflowStepKind::Agent);
        let output_configuration = configuration(WorkflowStepKind::Output);
        let step = |id: &str,
                    kind: WorkflowStepKind,
                    configuration: &WorkflowPayload,
                    capability| WorkflowStepSpec {
            id: id.into(),
            label: id.into(),
            kind,
            configuration_digest: configuration.digest().clone(),
            input_schema_digest: data_schema.digest().clone(),
            output_schema_digest: data_schema.digest().clone(),
            policy_digest: None,
            capability,
        };
        let workflow = WorkflowSpec {
            name: "Historic provider workflow".into(),
            description: String::new(),
            steps: vec![
                step("input", WorkflowStepKind::Input, &input_configuration, None),
                step(
                    "agent",
                    WorkflowStepKind::Agent,
                    &agent_configuration,
                    Some(CapabilityReference {
                        owner: CapabilityType::AgentRelease.owner(),
                        capability_type: CapabilityType::AgentRelease,
                        resource_id: Uuid::now_v7(),
                        revision: "release-1".into(),
                        digest: digest('d'),
                        capability: "agent.invoke".into(),
                    }),
                ),
                step(
                    "output",
                    WorkflowStepKind::Output,
                    &output_configuration,
                    None,
                ),
            ],
            edges: vec![
                WorkflowEdgeSpec {
                    id: "input-agent".into(),
                    source: "input".into(),
                    target: "agent".into(),
                    source_handle: None,
                },
                WorkflowEdgeSpec {
                    id: "agent-output".into(),
                    source: "agent".into(),
                    target: "output".into(),
                    source_handle: None,
                },
            ],
        };
        let contract = WorkflowContract::from_spec(workflow.clone()).expect("workflow contract");
        let revision = WorkflowRevision::initial(
            organization_id,
            project_id,
            definition_id,
            revision_id,
            contract.clone(),
            vec![
                data_schema,
                input_configuration,
                agent_configuration,
                output_configuration,
            ],
            principal_id,
            now,
        )
        .expect("historic revision remains structurally readable");
        let definition = WorkflowDefinition::create(
            organization_id,
            project_id,
            definition_id,
            workflow.name.clone(),
            workflow.description.clone(),
            revision_id,
            contract.digest().clone(),
            principal_id,
            now,
        )
        .expect("definition");
        let ontology_id = OntologyId::new();
        let ontology_revision_id = OntologyRevisionId::new();
        let ontology_contract = OntologyContract::from_spec(OntologySpec {
            name: "Historic provider ontology".into(),
            description: String::new(),
            object_types: vec![OntologyObjectType {
                id: "request".into(),
                label: "Request".into(),
                schema_digest: digest('e'),
                key_fields: vec!["id".into()],
            }],
            relation_types: Vec::new(),
            rules: Vec::new(),
        })
        .expect("ontology contract");
        let ontology_revision = OntologyRevision::initial(
            organization_id,
            project_id,
            ontology_id,
            ontology_revision_id,
            ontology_contract.clone(),
            principal_id,
            now,
        );
        let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
            name: "Historic provider goal".into(),
            workflow_definition_id: definition_id,
            workflow_revision_id: revision_id,
            workflow_digest: contract.digest().clone(),
            ontology_id,
            ontology_revision_id,
            ontology_digest: ontology_contract.digest().clone(),
            environment_id: None,
            input: json!({}),
        })
        .expect("goal contract");

        let plan_error = WorkflowPlanCompiler::compile_goal(
            WorkflowGoalId::new(),
            PlanRevisionId::new(),
            goal_contract.clone(),
            &definition,
            &revision,
            &ontology_revision,
            principal_id,
            now,
        )
        .expect_err("historic unwired revision must not compile a new Plan");
        assert!(
            plan_error.contains("has no admitted Cloud runtime dispatch port"),
            "unexpected Plan admission error: {plan_error}"
        );

        let goal_id = WorkflowGoalId::new();
        let plan_revision = PlanRevision::create(
            organization_id,
            project_id,
            goal_id,
            PlanRevisionId::new(),
            WorkflowPlan {
                schema: WORKFLOW_PLAN_SCHEMA.into(),
                compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION.into(),
                workflow_definition_id: definition_id,
                workflow_revision_id: revision_id,
                workflow_digest: contract.digest().clone(),
                workflow_payload_set_digest: revision.payload_set_digest.clone(),
                semantic_contract_set_digest: None,
                variable_contract_digest: None,
                composite_regions_digest: None,
                ontology_id,
                ontology_revision_id,
                ontology_digest: ontology_contract.digest().clone(),
                environment_id: None,
                input_digest: goal_contract.input_digest().clone(),
                steps: workflow
                    .steps
                    .iter()
                    .map(|step| WorkflowPlanStep {
                        id: step.id.clone(),
                        kind: step.kind,
                        configuration_digest: step.configuration_digest.clone(),
                        input_schema_digest: step.input_schema_digest.clone(),
                        output_schema_digest: step.output_schema_digest.clone(),
                        policy_digest: step.policy_digest.clone(),
                        capability: step.capability.clone(),
                        descriptor: None,
                        failure: None,
                        default_output: None,
                    })
                    .collect(),
                edges: workflow.edges.clone(),
            },
            principal_id,
            now,
        )
        .expect("historic Plan remains readable");
        let goal = WorkflowGoal::create(
            organization_id,
            project_id,
            goal_id,
            goal_contract,
            &plan_revision,
            principal_id,
            now,
        )
        .expect("historic Goal remains readable");
        let run_error = WorkflowRunCompiler::compile(
            WorkflowRunId::new(),
            &goal,
            &plan_revision,
            &revision,
            None,
            principal_id,
            now,
        )
        .expect_err("historic unwired revision must not compile a new Run");
        assert!(
            run_error.contains("has no admitted Cloud runtime dispatch port"),
            "unexpected Run admission error: {run_error}"
        );
    }
}

#[cfg(test)]
mod retry_policy_tests {
    use super::*;
    use crate::modules::workflow::domain::{
        CapabilityOwner, CapabilityReference, WorkflowCancellationCompensation, WorkflowEdgeSpec,
        WorkflowPolicyMode, WorkflowRetryPolicy,
    };
    use uuid::Uuid;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn step(connector: bool) -> WorkflowStepSpec {
        WorkflowStepSpec {
            id: "invoke".into(),
            label: "Invoke".into(),
            kind: WorkflowStepKind::Service,
            configuration_digest: digest('a'),
            input_schema_digest: digest('b'),
            output_schema_digest: digest('c'),
            policy_digest: Some(digest('d')),
            capability: connector.then(|| CapabilityReference {
                owner: CapabilityOwner::Connectors,
                capability_type: CapabilityType::ConnectorRevision,
                resource_id: Uuid::now_v7(),
                revision: Uuid::now_v7().to_string(),
                digest: digest('e'),
                capability: "connector.http".into(),
            }),
        }
    }

    fn policy(retry: Option<WorkflowRetryPolicy>) -> WorkflowPolicy {
        WorkflowPolicy {
            mode: WorkflowPolicyMode::Static,
            expression: None,
            candidates: Vec::new(),
            retry,
            default_output: None,
            cancellation_compensation: None,
        }
    }

    fn connector_step(id: &str, input: Sha256Digest, output: Sha256Digest) -> WorkflowStepSpec {
        let mut value = step(true);
        value.id = id.into();
        value.label = id.into();
        value.input_schema_digest = input;
        value.output_schema_digest = output;
        value
    }

    #[test]
    fn exact_retry_budget_is_required_only_for_connector_steps() {
        let retry = WorkflowRetryPolicy {
            maximum_attempts: 3,
            default_delay_seconds: 5,
        };
        assert!(validate_retry_policy_binding(&step(true), Some(&policy(Some(retry)))).is_ok());
        assert!(validate_retry_policy_binding(&step(true), None).is_err());
        assert!(validate_retry_policy_binding(&step(true), Some(&policy(None))).is_err());
        assert!(validate_retry_policy_binding(&step(false), Some(&policy(Some(retry)))).is_err());
        assert!(validate_retry_policy_binding(&step(false), Some(&policy(None))).is_ok());
    }

    #[test]
    fn cancellation_compensation_requires_one_downstream_exact_connector_route() {
        let schema = digest('b');
        let local_step =
            |id: &str, kind: WorkflowStepKind, schema: Sha256Digest| WorkflowStepSpec {
                id: id.into(),
                label: id.into(),
                kind,
                configuration_digest: digest('f'),
                input_schema_digest: schema.clone(),
                output_schema_digest: schema,
                policy_digest: None,
                capability: None,
            };
        let workflow = WorkflowSpec {
            name: "Cancellation compensation".into(),
            description: String::new(),
            steps: vec![
                local_step("input", WorkflowStepKind::Input, digest('a')),
                connector_step("reserve", digest('a'), schema.clone()),
                connector_step("release", schema.clone(), digest('c')),
                local_step("success_output", WorkflowStepKind::Output, schema.clone()),
                local_step("compensation_output", WorkflowStepKind::Output, digest('c')),
            ],
            edges: vec![
                WorkflowEdgeSpec {
                    id: "input-reserve".into(),
                    source: "input".into(),
                    target: "reserve".into(),
                    source_handle: None,
                },
                WorkflowEdgeSpec {
                    id: "reserve-success".into(),
                    source: "reserve".into(),
                    target: "success_output".into(),
                    source_handle: None,
                },
                WorkflowEdgeSpec {
                    id: "reserve-release".into(),
                    source: "reserve".into(),
                    target: "release".into(),
                    source_handle: Some("compensate".into()),
                },
                WorkflowEdgeSpec {
                    id: "release-compensation".into(),
                    source: "release".into(),
                    target: "compensation_output".into(),
                    source_handle: None,
                },
            ],
        };
        let mut source_policy = policy(Some(WorkflowRetryPolicy {
            maximum_attempts: 3,
            default_delay_seconds: 5,
        }));
        source_policy.cancellation_compensation = Some(WorkflowCancellationCompensation {
            step_id: "release".into(),
        });
        let target_policy = policy(Some(WorkflowRetryPolicy {
            maximum_attempts: 3,
            default_delay_seconds: 5,
        }));
        let policies = BTreeMap::from([("reserve", &source_policy), ("release", &target_policy)]);

        let validation = validate_cancellation_compensation_bindings(&workflow, &policies);
        assert!(validation.is_ok(), "{validation:?}");

        let mut implicit_route = workflow.clone();
        implicit_route.edges[2].source_handle = None;
        assert!(validate_cancellation_compensation_bindings(&implicit_route, &policies).is_err());

        let mut incompatible = workflow;
        incompatible.steps[2].input_schema_digest = digest('d');
        assert!(validate_cancellation_compensation_bindings(&incompatible, &policies).is_err());
    }
}
