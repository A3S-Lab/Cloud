use super::{
    WorkflowPlan, WorkflowSpec, WorkflowStepBindingKind, WorkflowStepDescriptorBindings,
    WorkflowStepDescriptorRegistry, WorkflowStepOwner, WorkflowVariableContract,
    WorkflowVariableDefaults, WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA,
    WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA, WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
    WORKFLOW_VARIABLE_CONTRACT_SCHEMA, WORKFLOW_VARIABLE_DEFAULTS_SCHEMA,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkflowRevisionSemanticContractKind {
    DescriptorBindings,
    DescriptorRegistry,
    VariableContract,
    VariableDefaults,
}

impl WorkflowRevisionSemanticContractKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DescriptorBindings => "descriptor_bindings",
            Self::DescriptorRegistry => "descriptor_registry",
            Self::VariableContract => "variable_contract",
            Self::VariableDefaults => "variable_defaults",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "descriptor_bindings" => Ok(Self::DescriptorBindings),
            "descriptor_registry" => Ok(Self::DescriptorRegistry),
            "variable_contract" => Ok(Self::VariableContract),
            "variable_defaults" => Ok(Self::VariableDefaults),
            _ => Err(format!(
                "unsupported Workflow revision semantic contract kind {value:?}"
            )),
        }
    }
}

/// Immutable compiler inputs owned by one Workflow revision.
///
/// The registry is retained so every bound semantic digest is recoverable. Its
/// presentation and admission metadata are deliberately excluded from
/// `digest`, which is derived from the binding, variable, and optional
/// variable-default contracts only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRevisionSemanticContracts {
    descriptor_bindings: WorkflowStepDescriptorBindings,
    descriptor_registry: WorkflowStepDescriptorRegistry,
    variable_contract: WorkflowVariableContract,
    variable_defaults: Option<WorkflowVariableDefaults>,
    digest: Sha256Digest,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkflowRevisionSemanticContractRef<'a> {
    pub kind: WorkflowRevisionSemanticContractKind,
    pub schema: &'static str,
    pub canonical_acl: &'a str,
    pub digest: &'a Sha256Digest,
}

impl WorkflowRevisionSemanticContracts {
    pub fn create(
        workflow: &WorkflowSpec,
        descriptor_bindings: WorkflowStepDescriptorBindings,
        descriptor_registry: WorkflowStepDescriptorRegistry,
        variable_contract: WorkflowVariableContract,
    ) -> Result<Self, String> {
        Self::create_with_defaults(
            workflow,
            descriptor_bindings,
            descriptor_registry,
            variable_contract,
            None,
        )
    }

    pub fn create_with_defaults(
        workflow: &WorkflowSpec,
        descriptor_bindings: WorkflowStepDescriptorBindings,
        descriptor_registry: WorkflowStepDescriptorRegistry,
        variable_contract: WorkflowVariableContract,
        variable_defaults: Option<WorkflowVariableDefaults>,
    ) -> Result<Self, String> {
        validate_default_material(&variable_contract, variable_defaults.as_ref())?;
        let digest = digest_contract_set(
            &descriptor_bindings,
            &variable_contract,
            variable_defaults.as_ref(),
        )?;
        let value = Self {
            descriptor_bindings,
            descriptor_registry,
            variable_contract,
            variable_defaults,
            digest,
        };
        value.validate(workflow)?;
        Ok(value)
    }

    pub fn restore(
        workflow: &WorkflowSpec,
        descriptor_bindings_acl: &str,
        descriptor_bindings_digest: &str,
        descriptor_registry_acl: &str,
        descriptor_registry_digest: &str,
        variable_contract_acl: &str,
        variable_contract_digest: &str,
    ) -> Result<Self, String> {
        Self::restore_with_defaults(
            workflow,
            descriptor_bindings_acl,
            descriptor_bindings_digest,
            descriptor_registry_acl,
            descriptor_registry_digest,
            variable_contract_acl,
            variable_contract_digest,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore_with_defaults(
        workflow: &WorkflowSpec,
        descriptor_bindings_acl: &str,
        descriptor_bindings_digest: &str,
        descriptor_registry_acl: &str,
        descriptor_registry_digest: &str,
        variable_contract_acl: &str,
        variable_contract_digest: &str,
        variable_defaults: Option<(&str, &str)>,
    ) -> Result<Self, String> {
        let descriptor_bindings = WorkflowStepDescriptorBindings::restore(
            descriptor_bindings_acl,
            descriptor_bindings_digest,
        )?;
        let descriptor_registry = WorkflowStepDescriptorRegistry::restore(
            descriptor_registry_acl,
            descriptor_registry_digest,
        )?;
        let variable_contract =
            WorkflowVariableContract::restore(variable_contract_acl, variable_contract_digest)?;
        let variable_defaults = variable_defaults
            .map(|(acl, digest)| WorkflowVariableDefaults::restore(acl, digest))
            .transpose()?;
        if variable_defaults.is_some() {
            return Self::create_with_defaults(
                workflow,
                descriptor_bindings,
                descriptor_registry,
                variable_contract,
                variable_defaults,
            );
        }

        // Schema-2 revisions published before migration 107 could retain a
        // digest-only default declaration without its bytes. They remain
        // readable with their original semantic-set digest, while Run
        // compilation still fails closed until a successor revision supplies
        // exact immutable material.
        let digest = digest_contract_set(&descriptor_bindings, &variable_contract, None)?;
        let value = Self {
            descriptor_bindings,
            descriptor_registry,
            variable_contract,
            variable_defaults: None,
            digest,
        };
        value.validate(workflow)?;
        Ok(value)
    }

    pub fn validate(&self, workflow: &WorkflowSpec) -> Result<(), String> {
        if self.descriptor_bindings.compiler_schema_version()
            != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
            || self.descriptor_registry.compiler_schema_version()
                != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
            || self.variable_contract.compiler_schema_version()
                != WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION
            || digest_contract_set(
                &self.descriptor_bindings,
                &self.variable_contract,
                self.variable_defaults.as_ref(),
            )? != self.digest
        {
            return Err("Workflow semantic contract compiler authority is invalid".into());
        }
        if let Some(defaults) = self.variable_defaults.as_ref() {
            defaults.validate_contract(&self.variable_contract)?;
        }
        workflow.topological_order(Default::default())?;
        if self.descriptor_bindings.bindings().len() != workflow.steps.len() {
            return Err(
                "Workflow descriptor bindings must cover every graph step exactly once".into(),
            );
        }

        let mut referenced_descriptors = BTreeSet::new();
        let mut application_ports = BTreeSet::new();
        let mut descriptors_by_step = BTreeMap::new();
        for step in &workflow.steps {
            let binding = self
                .descriptor_bindings
                .resolve(&step.id)
                .ok_or_else(|| format!("Workflow step {:?} has no descriptor binding", step.id))?;
            let descriptor = self.descriptor_registry.resolve_for_compiler(
                &binding.descriptor_id,
                &binding.descriptor_revision,
                WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            )?;
            if descriptor.semantic_digest() != &binding.semantic_digest
                || descriptor.spec().kind != Some(step.kind)
            {
                return Err(format!(
                    "Workflow step {:?} descriptor semantics do not match its graph kind",
                    step.id
                ));
            }
            validate_supported_bindings(step, descriptor.spec())?;
            validate_capability_binding(step, descriptor.spec())?;
            descriptors_by_step.insert(step.id.as_str(), descriptor.spec());
            referenced_descriptors.insert((descriptor.id(), descriptor.revision()));
            if descriptor.spec().owner == WorkflowStepOwner::Applications {
                application_ports.insert(step.id.as_str());
            }
        }
        let stored_descriptors = self
            .descriptor_registry
            .descriptors()
            .iter()
            .map(|descriptor| (descriptor.id(), descriptor.revision()))
            .collect::<BTreeSet<_>>();
        if referenced_descriptors != stored_descriptors {
            return Err(
                "Workflow descriptor registry snapshot must contain exactly the bound revisions"
                    .into(),
            );
        }
        validate_variable_read_ports(self.variable_contract.spec(), &descriptors_by_step)?;
        self.variable_contract
            .validate_graph_bindings_with_application_ports(workflow, &application_ports)
    }

    pub const fn descriptor_bindings(&self) -> &WorkflowStepDescriptorBindings {
        &self.descriptor_bindings
    }

    pub const fn descriptor_registry(&self) -> &WorkflowStepDescriptorRegistry {
        &self.descriptor_registry
    }

    pub const fn variable_contract(&self) -> &WorkflowVariableContract {
        &self.variable_contract
    }

    pub const fn variable_defaults(&self) -> Option<&WorkflowVariableDefaults> {
        self.variable_defaults.as_ref()
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub(crate) fn validate_plan_bindings(&self, plan: &WorkflowPlan) -> Result<(), String> {
        if plan.semantic_contract_set_digest.as_ref() != Some(&self.digest)
            || plan.variable_contract_digest.as_ref() != Some(self.variable_contract.digest())
            || plan.steps.len() != self.descriptor_bindings.bindings().len()
        {
            return Err("Workflow plan semantic contract authority drifted".into());
        }
        for step in &plan.steps {
            let expected = self
                .descriptor_bindings
                .resolve(&step.id)
                .ok_or_else(|| format!("Workflow plan step {:?} lost its descriptor", step.id))?;
            if step.descriptor.as_ref() != Some(expected) {
                return Err(format!(
                    "Workflow plan step {:?} descriptor authority drifted",
                    step.id
                ));
            }
        }
        Ok(())
    }

    pub fn persisted_contracts(&self) -> Vec<WorkflowRevisionSemanticContractRef<'_>> {
        let mut values = vec![
            WorkflowRevisionSemanticContractRef {
                kind: WorkflowRevisionSemanticContractKind::DescriptorBindings,
                schema: WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA,
                canonical_acl: self.descriptor_bindings.canonical_acl(),
                digest: self.descriptor_bindings.digest(),
            },
            WorkflowRevisionSemanticContractRef {
                kind: WorkflowRevisionSemanticContractKind::DescriptorRegistry,
                schema: WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA,
                canonical_acl: self.descriptor_registry.canonical_acl(),
                digest: self.descriptor_registry.digest(),
            },
            WorkflowRevisionSemanticContractRef {
                kind: WorkflowRevisionSemanticContractKind::VariableContract,
                schema: WORKFLOW_VARIABLE_CONTRACT_SCHEMA,
                canonical_acl: self.variable_contract.canonical_acl(),
                digest: self.variable_contract.digest(),
            },
        ];
        if let Some(defaults) = &self.variable_defaults {
            values.push(WorkflowRevisionSemanticContractRef {
                kind: WorkflowRevisionSemanticContractKind::VariableDefaults,
                schema: WORKFLOW_VARIABLE_DEFAULTS_SCHEMA,
                canonical_acl: defaults.canonical_acl(),
                digest: defaults.digest(),
            });
        }
        values
    }

    pub fn requires_binding(&self, kind: WorkflowStepBindingKind) -> bool {
        self.descriptor_bindings.bindings().iter().any(|binding| {
            self.descriptor_registry
                .resolve(&binding.descriptor_id, &binding.descriptor_revision)
                .is_some_and(|descriptor| descriptor.spec().required_bindings.contains(&kind))
        })
    }
}

fn validate_variable_read_ports(
    variables: &super::WorkflowVariableContractSpec,
    descriptors: &BTreeMap<&str, &super::WorkflowStepDescriptorSpec>,
) -> Result<(), String> {
    for read in &variables.reads {
        let descriptor = descriptors
            .get(read.consumer_step_id.as_str())
            .ok_or_else(|| {
                format!(
                    "Workflow variable read {:?} has no consumer descriptor",
                    read.id
                )
            })?;
        let port = descriptor
            .input_ports
            .iter()
            .find(|port| port.name == read.target_port)
            .ok_or_else(|| {
                format!(
                    "Workflow variable read {:?} targets undeclared descriptor input {:?}",
                    read.id, read.target_port
                )
            })?;
        if port.value_type != super::WorkflowDataType::Any && port.value_type != read.expected_type
        {
            return Err(format!(
                "Workflow variable read {:?} type does not match descriptor input {:?}",
                read.id, read.target_port
            ));
        }
    }
    Ok(())
}

fn validate_supported_bindings(
    step: &super::WorkflowStepSpec,
    descriptor: &super::WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    if let Some(unsupported) = descriptor.required_bindings.iter().find(|binding| {
        !matches!(
            binding,
            WorkflowStepBindingKind::CapabilityReference | WorkflowStepBindingKind::PlacementPolicy
        )
    }) {
        return Err(format!(
            "Workflow step {:?} descriptor requires unsupported {} binding",
            step.id,
            unsupported.as_str()
        ));
    }
    Ok(())
}

fn validate_capability_binding(
    step: &super::WorkflowStepSpec,
    descriptor: &super::WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    let requires_capability = descriptor
        .required_bindings
        .contains(&WorkflowStepBindingKind::CapabilityReference);
    match (requires_capability, step.capability.as_ref()) {
        (true, Some(capability))
            if descriptor
                .allowed_capability_types
                .contains(&capability.capability_type) =>
        {
            Ok(())
        }
        (false, None) => Ok(()),
        _ => Err(format!(
            "Workflow step {:?} capability does not satisfy its descriptor",
            step.id
        )),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SemanticContractDigestInput<'a> {
    descriptor_bindings_digest: &'a str,
    variable_contract_digest: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    variable_defaults_digest: Option<&'a str>,
}

fn digest_contract_set(
    bindings: &WorkflowStepDescriptorBindings,
    variables: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
) -> Result<Sha256Digest, String> {
    let encoded = serde_json::to_vec(&SemanticContractDigestInput {
        descriptor_bindings_digest: bindings.digest().as_str(),
        variable_contract_digest: variables.digest().as_str(),
        variable_defaults_digest: defaults.map(|value| value.digest().as_str()),
    })
    .map_err(|error| format!("could not encode Workflow semantic contract set: {error}"))?;
    Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn validate_default_material(
    variables: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
) -> Result<(), String> {
    let requires_defaults = variables
        .spec()
        .declarations
        .iter()
        .any(|declaration| declaration.default_value_digest.is_some());
    match (requires_defaults, defaults) {
        (false, None) => Ok(()),
        (true, Some(defaults)) => defaults.validate_contract(variables),
        (true, None) => Err(
            "Workflow variable contract declares digest-only defaults without immutable material"
                .into(),
        ),
        (false, Some(_)) => {
            Err("Workflow variable defaults are present without digest-backed declarations".into())
        }
    }
}
