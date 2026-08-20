use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{
    CapabilityType, WorkflowContract, WorkflowPayload, WorkflowPayloadContent, WorkflowPayloadKind,
    WorkflowPolicy, WorkflowRevisionSemanticContracts, WorkflowStepKind, WorkflowStepSpec,
    WORKFLOW_DEFINITION_SCHEMA,
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
        validate_payload_bindings(&self.contract, &self.payloads)?;
        if let Some(contracts) = &self.semantic_contracts {
            contracts.validate(self.contract.spec())?;
        } else if self.contract.spec().has_non_branch_source_handles() {
            return Err(
                "Workflow failure routes require immutable descriptor semantic contracts".into(),
            );
        }
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
}

fn validate_payload_bindings(
    contract: &WorkflowContract,
    payloads: &[WorkflowPayload],
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
        for digest in [&step.input_schema_digest, &step.output_schema_digest] {
            require_payload(
                &by_digest,
                digest,
                WorkflowPayloadKind::DataSchema,
                &step.id,
            )?;
            referenced.insert(digest.clone());
        }
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
        if step.kind == WorkflowStepKind::Branch {
            validate_branch_handles(contract, &step.id, configuration)?;
        }
    }
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
            "Workflow Connector step {:?} requires an exact policy v2 retry budget",
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
    let outgoing = contract
        .spec()
        .edges
        .iter()
        .filter(|edge| edge.source == step_id)
        .filter_map(|edge| edge.source_handle.as_deref())
        .collect::<BTreeSet<_>>();
    if configured != outgoing {
        return Err(format!(
            "Workflow branch {step_id:?} routes do not exactly match its outgoing handles"
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
mod retry_policy_tests {
    use super::*;
    use crate::modules::workflow::domain::{
        CapabilityOwner, CapabilityReference, WorkflowPolicyMode, WorkflowRetryPolicy,
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
        }
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
}
