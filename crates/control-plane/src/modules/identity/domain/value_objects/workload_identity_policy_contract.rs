use super::trust_domain_contract::{
    acl_integer, digest_list, exact_child, normalize_source, require_exact_string, required_bool,
    required_digest, required_digest_list, required_string, required_string_list, required_u32,
    required_uuid, strict_block, TrustDomainContract, WorkloadIdentityFormat,
    WorkloadIdentityRevocationMode, MAX_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS,
    MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InstallationId, NodePoolId, OrganizationId, ProjectId, Sha256Digest,
    TrustDomainId, TrustDomainRevisionId, WorkloadId, WorkloadIdentityPolicyId, WorkloadRevisionId,
};
use a3s_acl::builder::{boolean, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const WORKLOAD_IDENTITY_POLICY_SCHEMA: &str = "cloud.identity.workload-policy.v1";
pub const WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES: usize = 64 * 1024;
pub const MAX_WORKLOAD_IDENTITY_AUDIENCES: usize = 16;
pub const MAX_PRIVATE_SERVICE_NAMES: usize = 16;
pub const MAX_PEER_POLICY_REVISIONS: usize = 32;

const POLICY_BLOCK: &str = "workload_identity_policy";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkloadIdentityAudience(String);

impl WorkloadIdentityAudience {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 255
            || value != value.to_ascii_lowercase()
            || value.starts_with(['-', '.', '/', ':'])
            || value.ends_with(['-', '.', '/', ':'])
            || value.bytes().any(|byte| {
                !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-._:/".contains(&byte))
            })
        {
            return Err(
                "workload identity audience must be a bounded canonical lowercase identifier"
                    .into(),
            );
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrivateServiceName(String);

impl PrivateServiceName {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 253
            || value != value.to_ascii_lowercase()
            || value.starts_with('.')
            || value.ends_with('.')
            || value.contains("..")
            || value.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || !label.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
                    || label.starts_with('-')
                    || label.ends_with('-')
            })
        {
            return Err("private service name must be a canonical lowercase DNS name".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadProductRole {
    GenericTask,
    GenericService,
    AgentService,
    WorkflowWorker,
    FunctionTask,
    FunctionService,
    McpService,
    DurableCellService,
    InferenceRouter,
    InferenceWorker,
    BuildTask,
    CloudSystemService,
    Gateway,
}

impl WorkloadProductRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GenericTask => "generic_task",
            Self::GenericService => "generic_service",
            Self::AgentService => "agent_service",
            Self::WorkflowWorker => "workflow_worker",
            Self::FunctionTask => "function_task",
            Self::FunctionService => "function_service",
            Self::McpService => "mcp_service",
            Self::DurableCellService => "durable_cell_service",
            Self::InferenceRouter => "inference_router",
            Self::InferenceWorker => "inference_worker",
            Self::BuildTask => "build_task",
            Self::CloudSystemService => "cloud_system_service",
            Self::Gateway => "gateway",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "generic_task" => Ok(Self::GenericTask),
            "generic_service" => Ok(Self::GenericService),
            "agent_service" => Ok(Self::AgentService),
            "workflow_worker" => Ok(Self::WorkflowWorker),
            "function_task" => Ok(Self::FunctionTask),
            "function_service" => Ok(Self::FunctionService),
            "mcp_service" => Ok(Self::McpService),
            "durable_cell_service" => Ok(Self::DurableCellService),
            "inference_router" => Ok(Self::InferenceRouter),
            "inference_worker" => Ok(Self::InferenceWorker),
            "build_task" => Ok(Self::BuildTask),
            "cloud_system_service" => Ok(Self::CloudSystemService),
            "gateway" => Ok(Self::Gateway),
            _ => Err("workload product role is unsupported".into()),
        }
    }

    pub const fn required_runtime_class(self) -> RuntimeUnitClass {
        match self {
            Self::GenericTask | Self::FunctionTask | Self::BuildTask => RuntimeUnitClass::Task,
            Self::GenericService
            | Self::AgentService
            | Self::WorkflowWorker
            | Self::FunctionService
            | Self::McpService
            | Self::DurableCellService
            | Self::InferenceRouter
            | Self::InferenceWorker
            | Self::CloudSystemService
            | Self::Gateway => RuntimeUnitClass::Service,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadIdentityPolicySpec {
    pub installation_id: InstallationId,
    pub trust_domain_id: TrustDomainId,
    pub trust_domain_revision_id: TrustDomainRevisionId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub product_role: WorkloadProductRole,
    pub runtime_class: RuntimeUnitClass,
    pub semantics_profile_digest: Sha256Digest,
    pub node_pool_id: NodePoolId,
    pub isolation_level: RuntimeIsolationLevel,
    pub attestation_profile_digest: Sha256Digest,
    pub confidential_compute: bool,
    pub identity_formats: Vec<WorkloadIdentityFormat>,
    pub credential_lifetime_seconds: u32,
    pub rotate_before_expiry_seconds: u32,
    pub drain_on_rotation_failure: bool,
    pub revoke_on_stop: bool,
    pub audiences: Vec<WorkloadIdentityAudience>,
    pub service_names: Vec<PrivateServiceName>,
    pub peer_policy_revision_digests: Vec<Sha256Digest>,
}

impl WorkloadIdentityPolicySpec {
    fn normalize(mut self) -> Result<Self, String> {
        normalize_unique(&mut self.identity_formats, 1, 2, "identity formats")?;
        normalize_unique(
            &mut self.audiences,
            1,
            MAX_WORKLOAD_IDENTITY_AUDIENCES,
            "audiences",
        )?;
        normalize_unique(
            &mut self.service_names,
            0,
            MAX_PRIVATE_SERVICE_NAMES,
            "private service names",
        )?;
        normalize_unique(
            &mut self.peer_policy_revision_digests,
            0,
            MAX_PEER_POLICY_REVISIONS,
            "peer policy revisions",
        )?;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if [
            self.installation_id.as_uuid(),
            self.trust_domain_id.as_uuid(),
            self.trust_domain_revision_id.as_uuid(),
            self.organization_id.as_uuid(),
            self.project_id.as_uuid(),
            self.environment_id.as_uuid(),
            self.policy_id.as_uuid(),
            self.workload_id.as_uuid(),
            self.workload_revision_id.as_uuid(),
            self.node_pool_id.as_uuid(),
        ]
        .contains(&uuid::Uuid::nil())
            || self.runtime_class != self.product_role.required_runtime_class()
            || self.confidential_compute
                != (self.isolation_level == RuntimeIsolationLevel::Confidential)
            || !(MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS
                ..=MAX_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS)
                .contains(&self.credential_lifetime_seconds)
            || self.rotate_before_expiry_seconds < 15
            || self.rotate_before_expiry_seconds > self.credential_lifetime_seconds / 2
        {
            return Err("workload identity policy identity or lifecycle bounds are invalid".into());
        }
        validate_digest(&self.semantics_profile_digest, "semantics profile")?;
        validate_digest(&self.attestation_profile_digest, "attestation profile")?;
        validate_canonical_set(&self.identity_formats, 1, 2, "identity formats")?;
        validate_canonical_set(
            &self.audiences,
            1,
            MAX_WORKLOAD_IDENTITY_AUDIENCES,
            "audiences",
        )?;
        validate_canonical_set(
            &self.service_names,
            0,
            MAX_PRIVATE_SERVICE_NAMES,
            "private service names",
        )?;
        validate_canonical_set(
            &self.peer_policy_revision_digests,
            0,
            MAX_PEER_POLICY_REVISIONS,
            "peer policy revisions",
        )?;
        for audience in &self.audiences {
            WorkloadIdentityAudience::parse(audience.as_str())?;
        }
        for service_name in &self.service_names {
            PrivateServiceName::parse(service_name.as_str())?;
        }
        for digest in &self.peer_policy_revision_digests {
            validate_digest(digest, "peer policy revision")?;
        }
        match self.runtime_class {
            RuntimeUnitClass::Task if !self.service_names.is_empty() => {
                return Err("Task workload identity policy cannot publish a private service".into())
            }
            RuntimeUnitClass::Service
                if self.service_names.is_empty()
                    || !self
                        .identity_formats
                        .contains(&WorkloadIdentityFormat::X509Svid) =>
            {
                return Err(
                    "Service workload identity policy requires a private service and X.509 SVID"
                        .into(),
                )
            }
            _ => {}
        }
        Ok(())
    }

    pub fn validate_against_trust_domain(
        &self,
        trust_domain: &TrustDomainContract,
    ) -> Result<(), String> {
        self.validate()?;
        trust_domain.validate()?;
        let trust = trust_domain.spec();
        if self.installation_id != trust.installation_id
            || self.trust_domain_id != trust.trust_domain_id
            || self.credential_lifetime_seconds > trust.max_credential_lifetime_seconds
            || !self
                .identity_formats
                .iter()
                .all(|format| trust.identity_formats.contains(format))
            || !trust
                .node_attestation_profile_digests
                .contains(&self.attestation_profile_digest)
            || (self.revoke_on_stop
                && trust.revocation_mode != WorkloadIdentityRevocationMode::EpochAndExpiry)
        {
            return Err("workload identity policy is not admitted by its trust domain".into());
        }
        Ok(())
    }
}

/// Canonical Identity-owned policy for one exact logical Workload revision.
///
/// The contract carries no certificate, key, endpoint, Runtime observation,
/// node attestation document or network implementation. Those remain with the
/// provider and their owning bounded contexts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadIdentityPolicyContract {
    spec: WorkloadIdentityPolicySpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkloadIdentityPolicyContract {
    pub fn from_spec(spec: WorkloadIdentityPolicySpec) -> Result<Self, String> {
        let spec = spec.normalize()?;
        let document = contract_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES {
            return Err("workload identity policy ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated workload identity policy ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("workload identity policy ACL is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        let normalized = normalize_source(
            source,
            WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES,
            "workload identity policy",
        )?;
        let document = parse_acl(&normalized)
            .map_err(|error| format!("workload identity policy ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_contract(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("workload identity policy ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored workload identity policy ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("workload identity policy drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &WorkloadIdentityPolicySpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &WorkloadIdentityPolicySpec) -> Result<Document, String> {
    let placement = BlockBuilder::new("placement")
        .attr(
            "attestation_profile_digest",
            string(spec.attestation_profile_digest.as_str()),
        )
        .attr("confidential_compute", boolean(spec.confidential_compute))
        .attr(
            "isolation_level",
            string(isolation_level_name(spec.isolation_level)),
        )
        .attr("node_pool_id", string(&spec.node_pool_id.to_string()))
        .attr(
            "runtime_class",
            string(runtime_class_name(spec.runtime_class)),
        )
        .build();
    let credential = BlockBuilder::new("credential")
        .attr(
            "audiences",
            list(
                spec.audiences
                    .iter()
                    .map(|value| string(value.as_str()))
                    .collect(),
            ),
        )
        .attr(
            "credential_lifetime_seconds",
            acl_integer(
                "credential_lifetime_seconds",
                u64::from(spec.credential_lifetime_seconds),
                false,
            )?,
        )
        .attr(
            "drain_on_rotation_failure",
            boolean(spec.drain_on_rotation_failure),
        )
        .attr(
            "identity_formats",
            list(
                spec.identity_formats
                    .iter()
                    .map(|format| string(format.as_str()))
                    .collect(),
            ),
        )
        .attr("revoke_on_stop", boolean(spec.revoke_on_stop))
        .attr(
            "rotate_before_expiry_seconds",
            acl_integer(
                "rotate_before_expiry_seconds",
                u64::from(spec.rotate_before_expiry_seconds),
                false,
            )?,
        )
        .attr(
            "service_names",
            list(
                spec.service_names
                    .iter()
                    .map(|value| string(value.as_str()))
                    .collect(),
            ),
        )
        .build();
    let authorization = BlockBuilder::new("authorization")
        .attr(
            "peer_policy_revision_digests",
            digest_list(&spec.peer_policy_revision_digests),
        )
        .build();
    Ok(Document {
        blocks: vec![BlockBuilder::new(POLICY_BLOCK)
            .attr("environment_id", string(&spec.environment_id.to_string()))
            .attr("installation_id", string(&spec.installation_id.to_string()))
            .attr("organization_id", string(&spec.organization_id.to_string()))
            .attr("policy_id", string(&spec.policy_id.to_string()))
            .attr("product_role", string(spec.product_role.as_str()))
            .attr("project_id", string(&spec.project_id.to_string()))
            .attr("schema", string(WORKLOAD_IDENTITY_POLICY_SCHEMA))
            .attr(
                "semantics_profile_digest",
                string(spec.semantics_profile_digest.as_str()),
            )
            .attr("trust_domain_id", string(&spec.trust_domain_id.to_string()))
            .attr(
                "trust_domain_revision_id",
                string(&spec.trust_domain_revision_id.to_string()),
            )
            .attr("workload_id", string(&spec.workload_id.to_string()))
            .attr(
                "workload_revision_id",
                string(&spec.workload_revision_id.to_string()),
            )
            .nested_block(authorization)
            .nested_block(credential)
            .nested_block(placement)
            .build()],
    })
}

fn parse_contract(document: &Document) -> Result<WorkloadIdentityPolicySpec, String> {
    if document.blocks.len() != 1 {
        return Err("workload identity policy ACL must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    strict_block(
        root,
        POLICY_BLOCK,
        &[
            "environment_id",
            "installation_id",
            "organization_id",
            "policy_id",
            "product_role",
            "project_id",
            "schema",
            "semantics_profile_digest",
            "trust_domain_id",
            "trust_domain_revision_id",
            "workload_id",
            "workload_revision_id",
        ],
        &["authorization", "credential", "placement"],
    )?;
    require_exact_string(root, "schema", WORKLOAD_IDENTITY_POLICY_SCHEMA)?;
    let placement = exact_child(root, "placement")?;
    strict_block(
        placement,
        "placement",
        &[
            "attestation_profile_digest",
            "confidential_compute",
            "isolation_level",
            "node_pool_id",
            "runtime_class",
        ],
        &[],
    )?;
    let credential = exact_child(root, "credential")?;
    strict_block(
        credential,
        "credential",
        &[
            "audiences",
            "credential_lifetime_seconds",
            "drain_on_rotation_failure",
            "identity_formats",
            "revoke_on_stop",
            "rotate_before_expiry_seconds",
            "service_names",
        ],
        &[],
    )?;
    let authorization = exact_child(root, "authorization")?;
    strict_block(
        authorization,
        "authorization",
        &["peer_policy_revision_digests"],
        &[],
    )?;
    Ok(WorkloadIdentityPolicySpec {
        installation_id: InstallationId::from_uuid(required_uuid(root, "installation_id")?),
        trust_domain_id: TrustDomainId::from_uuid(required_uuid(root, "trust_domain_id")?),
        trust_domain_revision_id: TrustDomainRevisionId::from_uuid(required_uuid(
            root,
            "trust_domain_revision_id",
        )?),
        organization_id: OrganizationId::from_uuid(required_uuid(root, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(root, "project_id")?),
        environment_id: EnvironmentId::from_uuid(required_uuid(root, "environment_id")?),
        policy_id: WorkloadIdentityPolicyId::from_uuid(required_uuid(root, "policy_id")?),
        workload_id: WorkloadId::from_uuid(required_uuid(root, "workload_id")?),
        workload_revision_id: WorkloadRevisionId::from_uuid(required_uuid(
            root,
            "workload_revision_id",
        )?),
        product_role: WorkloadProductRole::parse(&required_string(root, "product_role")?)?,
        runtime_class: parse_runtime_class(&required_string(placement, "runtime_class")?)?,
        semantics_profile_digest: required_digest(root, "semantics_profile_digest")?,
        node_pool_id: NodePoolId::from_uuid(required_uuid(placement, "node_pool_id")?),
        isolation_level: parse_isolation_level(&required_string(placement, "isolation_level")?)?,
        attestation_profile_digest: required_digest(placement, "attestation_profile_digest")?,
        confidential_compute: required_bool(placement, "confidential_compute")?,
        identity_formats: required_string_list(credential, "identity_formats")?
            .iter()
            .map(|value| WorkloadIdentityFormat::parse(value))
            .collect::<Result<Vec<_>, _>>()?,
        credential_lifetime_seconds: required_u32(
            credential,
            "credential_lifetime_seconds",
            false,
        )?,
        rotate_before_expiry_seconds: required_u32(
            credential,
            "rotate_before_expiry_seconds",
            false,
        )?,
        drain_on_rotation_failure: required_bool(credential, "drain_on_rotation_failure")?,
        revoke_on_stop: required_bool(credential, "revoke_on_stop")?,
        audiences: required_string_list(credential, "audiences")?
            .into_iter()
            .map(WorkloadIdentityAudience::parse)
            .collect::<Result<Vec<_>, _>>()?,
        service_names: required_string_list(credential, "service_names")?
            .into_iter()
            .map(PrivateServiceName::parse)
            .collect::<Result<Vec<_>, _>>()?,
        peer_policy_revision_digests: required_digest_list(
            authorization,
            "peer_policy_revision_digests",
        )?,
    })
}

fn runtime_class_name(value: RuntimeUnitClass) -> &'static str {
    match value {
        RuntimeUnitClass::Task => "task",
        RuntimeUnitClass::Service => "service",
    }
}

fn parse_runtime_class(value: &str) -> Result<RuntimeUnitClass, String> {
    match value {
        "task" => Ok(RuntimeUnitClass::Task),
        "service" => Ok(RuntimeUnitClass::Service),
        _ => Err("workload identity Runtime class is unsupported".into()),
    }
}

fn isolation_level_name(value: RuntimeIsolationLevel) -> &'static str {
    match value {
        RuntimeIsolationLevel::Process => "process",
        RuntimeIsolationLevel::Container => "container",
        RuntimeIsolationLevel::Sandbox => "sandbox",
        RuntimeIsolationLevel::Confidential => "confidential",
    }
}

fn parse_isolation_level(value: &str) -> Result<RuntimeIsolationLevel, String> {
    match value {
        "process" => Ok(RuntimeIsolationLevel::Process),
        "container" => Ok(RuntimeIsolationLevel::Container),
        "sandbox" => Ok(RuntimeIsolationLevel::Sandbox),
        "confidential" => Ok(RuntimeIsolationLevel::Confidential),
        _ => Err("workload identity isolation level is unsupported".into()),
    }
}

fn validate_digest(value: &Sha256Digest, label: &str) -> Result<(), String> {
    if Sha256Digest::parse(value.as_str())? != *value {
        return Err(format!(
            "workload identity policy {label} digest is not canonical"
        ));
    }
    Ok(())
}

fn normalize_unique<T: Clone + Ord>(
    values: &mut Vec<T>,
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    if values.len() < minimum || values.len() > maximum {
        return Err(format!(
            "workload identity policy {label} count is outside bounds"
        ));
    }
    let unique = values.iter().cloned().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err(format!(
            "workload identity policy {label} contain duplicates"
        ));
    }
    *values = unique.into_iter().collect();
    Ok(())
}

fn validate_canonical_set<T: Clone + Ord + PartialEq>(
    values: &[T],
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    let mut normalized = values.to_vec();
    normalize_unique(&mut normalized, minimum, maximum, label)?;
    if normalized != values {
        return Err(format!(
            "workload identity policy {label} are not canonical"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        TrustDomainContractSpec, TrustDomainName,
    };

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn trust(
        installation_id: InstallationId,
        trust_domain_id: TrustDomainId,
    ) -> TrustDomainContract {
        TrustDomainContract::from_spec(TrustDomainContractSpec {
            installation_id,
            trust_domain_id,
            name: TrustDomainName::parse("prod.example.internal").expect("name"),
            provider_profile_digest: digest('a'),
            trust_bundle_digest: digest('b'),
            node_attestation_profile_digests: vec![digest('c')],
            identity_formats: vec![
                WorkloadIdentityFormat::X509Svid,
                WorkloadIdentityFormat::JwtSvid,
            ],
            max_credential_lifetime_seconds: 900,
            rotation_overlap_seconds: 120,
            revocation_mode: WorkloadIdentityRevocationMode::EpochAndExpiry,
            federation_bundle_digests: vec![],
        })
        .expect("trust domain")
    }

    fn service_spec(
        installation_id: InstallationId,
        trust_domain_id: TrustDomainId,
        trust_domain_revision_id: TrustDomainRevisionId,
    ) -> WorkloadIdentityPolicySpec {
        WorkloadIdentityPolicySpec {
            installation_id,
            trust_domain_id,
            trust_domain_revision_id,
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            policy_id: WorkloadIdentityPolicyId::new(),
            workload_id: WorkloadId::new(),
            workload_revision_id: WorkloadRevisionId::new(),
            product_role: WorkloadProductRole::AgentService,
            runtime_class: RuntimeUnitClass::Service,
            semantics_profile_digest: digest('d'),
            node_pool_id: NodePoolId::new(),
            isolation_level: RuntimeIsolationLevel::Container,
            attestation_profile_digest: digest('c'),
            confidential_compute: false,
            identity_formats: vec![
                WorkloadIdentityFormat::JwtSvid,
                WorkloadIdentityFormat::X509Svid,
            ],
            credential_lifetime_seconds: 600,
            rotate_before_expiry_seconds: 120,
            drain_on_rotation_failure: true,
            revoke_on_stop: true,
            audiences: vec![WorkloadIdentityAudience::parse("model.internal").expect("audience")],
            service_names: vec![PrivateServiceName::parse("agent.prod.internal").expect("service")],
            peer_policy_revision_digests: vec![digest('f'), digest('e')],
        }
    }

    #[test]
    fn canonical_policy_round_trips_and_is_admitted_by_trust_domain() {
        let installation_id = InstallationId::new();
        let trust_domain_id = TrustDomainId::new();
        let trust = trust(installation_id, trust_domain_id);
        let trust_domain_revision_id = TrustDomainRevisionId::new();
        let contract = WorkloadIdentityPolicyContract::from_spec(service_spec(
            installation_id,
            trust_domain_id,
            trust_domain_revision_id,
        ))
        .expect("policy");
        contract
            .spec()
            .validate_against_trust_domain(&trust)
            .expect("trust admission");
        assert_eq!(
            WorkloadIdentityPolicyContract::parse_acl(contract.canonical_acl())
                .expect("round trip"),
            contract
        );
        contract.validate().expect("valid contract");
    }

    #[test]
    fn rejects_role_runtime_service_and_trust_drift() {
        let installation_id = InstallationId::new();
        let trust_domain_id = TrustDomainId::new();
        let trust = trust(installation_id, trust_domain_id);
        let trust_domain_revision_id = TrustDomainRevisionId::new();

        let mut wrong_class =
            service_spec(installation_id, trust_domain_id, trust_domain_revision_id);
        wrong_class.runtime_class = RuntimeUnitClass::Task;
        assert!(WorkloadIdentityPolicyContract::from_spec(wrong_class).is_err());

        let mut no_service_name =
            service_spec(installation_id, trust_domain_id, trust_domain_revision_id);
        no_service_name.service_names.clear();
        assert!(WorkloadIdentityPolicyContract::from_spec(no_service_name).is_err());

        let mut untrusted_attestation =
            service_spec(installation_id, trust_domain_id, trust_domain_revision_id);
        untrusted_attestation.attestation_profile_digest = digest('9');
        let policy = WorkloadIdentityPolicyContract::from_spec(untrusted_attestation)
            .expect("locally valid policy");
        assert!(policy.spec().validate_against_trust_domain(&trust).is_err());
    }

    #[test]
    fn task_policy_cannot_publish_service_names() {
        let installation_id = InstallationId::new();
        let trust_domain_id = TrustDomainId::new();
        let mut task = service_spec(
            installation_id,
            trust_domain_id,
            TrustDomainRevisionId::new(),
        );
        task.product_role = WorkloadProductRole::FunctionTask;
        task.runtime_class = RuntimeUnitClass::Task;
        assert!(WorkloadIdentityPolicyContract::from_spec(task.clone()).is_err());
        task.service_names.clear();
        assert!(WorkloadIdentityPolicyContract::from_spec(task).is_ok());
    }

    #[test]
    fn rejects_noncanonical_acl_and_selectors() {
        assert!(WorkloadIdentityAudience::parse("Model.Internal").is_err());
        assert!(WorkloadIdentityAudience::parse("model.*").is_err());
        assert!(PrivateServiceName::parse("agent..internal").is_err());

        let contract = WorkloadIdentityPolicyContract::from_spec(service_spec(
            InstallationId::new(),
            TrustDomainId::new(),
            TrustDomainRevisionId::new(),
        ))
        .expect("policy");
        assert!(
            WorkloadIdentityPolicyContract::parse_acl(contract.canonical_acl().trim_end()).is_err()
        );
    }
}
