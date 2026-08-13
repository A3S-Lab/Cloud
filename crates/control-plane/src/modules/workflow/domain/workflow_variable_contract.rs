use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::{canonical_digest, generate_acl, parse_acl};

mod codec;
mod graph;
mod model;
mod validation;

pub use model::{
    WorkflowVariableAssignment, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
    WorkflowVariableExport, WorkflowVariableMutationMode, WorkflowVariableRead,
    WorkflowVariableReadMode, WorkflowVariableScope, WorkflowVariableStorageClass,
};
use validation::normalize_contract_spec;

pub const WORKFLOW_VARIABLE_CONTRACT_SCHEMA: &str = "cloud.workflow.variable-contract.v1";
pub const WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION: u32 = 2;
pub const WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES: usize = 2 * 1024 * 1024;

/// Immutable, canonical variable semantics for one Workflow revision.
///
/// This contract declares compiler-visible values and their ownership. It is
/// not a variable store, event history, scheduler, or execution engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowVariableContract {
    spec: WorkflowVariableContractSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkflowVariableContract {
    pub fn from_spec(spec: WorkflowVariableContractSpec) -> Result<Self, String> {
        let spec = normalize_contract_spec(spec)?;
        let document = codec::contract_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES {
            return Err("Workflow variable contract ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated Workflow variable ACL is invalid: {error}"))?;
        let digest = digest_document(&reparsed)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES {
            return Err("Workflow variable contract ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("Workflow variable contract contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let contract = Self::from_spec(codec::parse_contract_spec(&normalized)?)?;
        if contract.canonical_acl != normalized {
            return Err("Workflow variable contract ACL is not canonical".into());
        }
        Ok(contract)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let contract = Self::parse_acl(source)?;
        if contract.digest.as_str() != stored_digest {
            return Err("stored Workflow variable contract and digest do not match".into());
        }
        Ok(contract)
    }

    pub fn id(&self) -> &str {
        &self.spec.id
    }

    pub fn revision(&self) -> &str {
        &self.spec.revision
    }

    pub const fn compiler_schema_version(&self) -> u32 {
        self.spec.compiler_schema_version
    }

    pub const fn spec(&self) -> &WorkflowVariableContractSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    /// Validates step identities, schema bindings, reachability, and required
    /// dominance against the immutable Workflow graph being compiled.
    pub fn validate_graph_bindings(&self, workflow: &super::WorkflowSpec) -> Result<(), String> {
        graph::validate_graph_bindings(&self.spec, workflow, &Default::default())
    }

    pub(crate) fn validate_graph_bindings_with_application_ports(
        &self,
        workflow: &super::WorkflowSpec,
        application_ports: &std::collections::BTreeSet<&str>,
    ) -> Result<(), String> {
        graph::validate_graph_bindings(&self.spec, workflow, application_ports)
    }
}

fn digest_document(document: &a3s_acl::Document) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(
        canonical_digest(document).map_err(|error| {
            format!("Workflow variable contract is not canonicalizable: {error}")
        })?,
    )
}
