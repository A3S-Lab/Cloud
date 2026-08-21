use super::workflow_composite_regions::is_exact_child_workflow_revision;
use super::{
    validate_descriptor_failure_routes, CapabilityType, WorkflowCompositeRegions, WorkflowPlan,
    WorkflowSpec, WorkflowStepBindingKind, WorkflowStepDefaultOutputContract,
    WorkflowStepDescriptorBindings, WorkflowStepDescriptorRegistry, WorkflowStepExecutionClass,
    WorkflowStepFallbackMode, WorkflowStepKind, WorkflowStepOwner, WorkflowStepPortCardinality,
    WorkflowStepRetryClassification, WorkflowVariableContract, WorkflowVariableDefaults,
    WORKFLOW_COMPOSITE_REGIONS_SCHEMA, WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA,
    WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA, WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
    WORKFLOW_VARIABLE_CONTRACT_SCHEMA, WORKFLOW_VARIABLE_DEFAULTS_SCHEMA,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkflowRevisionSemanticContractKind {
    CompositeRegions,
    DescriptorBindings,
    DescriptorRegistry,
    VariableContract,
    VariableDefaults,
}

impl WorkflowRevisionSemanticContractKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompositeRegions => "composite_regions",
            Self::DescriptorBindings => "descriptor_bindings",
            Self::DescriptorRegistry => "descriptor_registry",
            Self::VariableContract => "variable_contract",
            Self::VariableDefaults => "variable_defaults",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "composite_regions" => Ok(Self::CompositeRegions),
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
    composite_regions: Option<WorkflowCompositeRegions>,
    digest: Sha256Digest,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkflowRevisionSemanticContractRef<'a> {
    pub kind: WorkflowRevisionSemanticContractKind,
    pub schema: &'static str,
    pub canonical_acl: &'a str,
    pub digest: &'a Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowApplicationOutputSteps {
    pub final_output_step_id: String,
    pub answer_step_ids: BTreeSet<String>,
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
        Self::create_with_optional_contracts(
            workflow,
            descriptor_bindings,
            descriptor_registry,
            variable_contract,
            variable_defaults,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_with_optional_contracts(
        workflow: &WorkflowSpec,
        descriptor_bindings: WorkflowStepDescriptorBindings,
        descriptor_registry: WorkflowStepDescriptorRegistry,
        variable_contract: WorkflowVariableContract,
        variable_defaults: Option<WorkflowVariableDefaults>,
        composite_regions: Option<WorkflowCompositeRegions>,
    ) -> Result<Self, String> {
        validate_default_material(&variable_contract, variable_defaults.as_ref())?;
        let digest = digest_contract_set(
            &descriptor_bindings,
            &variable_contract,
            variable_defaults.as_ref(),
            composite_regions.as_ref(),
        )?;
        let value = Self {
            descriptor_bindings,
            descriptor_registry,
            variable_contract,
            variable_defaults,
            composite_regions,
            digest,
        };
        value.validate(workflow)?;
        value.validate_composite_region_material(workflow, true)?;
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
        Self::restore_with_optional_contracts(
            workflow,
            descriptor_bindings_acl,
            descriptor_bindings_digest,
            descriptor_registry_acl,
            descriptor_registry_digest,
            variable_contract_acl,
            variable_contract_digest,
            variable_defaults,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore_with_optional_contracts(
        workflow: &WorkflowSpec,
        descriptor_bindings_acl: &str,
        descriptor_bindings_digest: &str,
        descriptor_registry_acl: &str,
        descriptor_registry_digest: &str,
        variable_contract_acl: &str,
        variable_contract_digest: &str,
        variable_defaults: Option<(&str, &str)>,
        composite_regions: Option<(&str, &str)>,
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
        let composite_regions = composite_regions
            .map(|(acl, digest)| WorkflowCompositeRegions::restore(acl, digest))
            .transpose()?;

        // Pre-migration optional material remains readable with its original
        // semantic-set digest. New publication uses the strict constructor,
        // while Run admission still fails closed when required bytes are absent.
        let digest = digest_contract_set(
            &descriptor_bindings,
            &variable_contract,
            variable_defaults.as_ref(),
            composite_regions.as_ref(),
        )?;
        let value = Self {
            descriptor_bindings,
            descriptor_registry,
            variable_contract,
            variable_defaults,
            composite_regions,
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
                self.composite_regions.as_ref(),
            )? != self.digest
        {
            return Err("Workflow semantic contract compiler authority is invalid".into());
        }
        if let Some(defaults) = self.variable_defaults.as_ref() {
            defaults.validate_contract(&self.variable_contract)?;
        }
        if let Some(regions) = self.composite_regions.as_ref() {
            regions.validate_identity(&self.descriptor_bindings, &self.variable_contract)?;
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
        let mut failures_by_step = BTreeMap::new();
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
            validate_connector_retry_authority(step, descriptor.spec())?;
            validate_default_output_authority(step, descriptor.spec())?;
            descriptors_by_step.insert(step.id.as_str(), descriptor.spec());
            failures_by_step.insert(step.id.as_str(), &descriptor.spec().failure);
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
        validate_descriptor_failure_routes(workflow, &failures_by_step)?;
        validate_variable_read_ports(self.variable_contract.spec(), &descriptors_by_step)?;
        self.variable_contract
            .validate_graph_bindings_with_application_ports(workflow, &application_ports)?;
        self.validate_composite_region_material(workflow, false)
    }

    pub const fn descriptor_bindings(&self) -> &WorkflowStepDescriptorBindings {
        &self.descriptor_bindings
    }

    pub const fn descriptor_registry(&self) -> &WorkflowStepDescriptorRegistry {
        &self.descriptor_registry
    }

    pub(crate) fn has_application_owned_steps(
        &self,
        workflow: &WorkflowSpec,
    ) -> Result<bool, String> {
        self.validate(workflow)?;
        for step in &workflow.steps {
            if self.descriptor_for_step(&step.id)?.spec().owner == WorkflowStepOwner::Applications {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn application_output_steps(
        &self,
        workflow: &WorkflowSpec,
    ) -> Result<WorkflowApplicationOutputSteps, String> {
        self.validate(workflow)?;
        let mut final_output_step_ids = Vec::new();
        let mut answer_step_ids = BTreeSet::new();
        for step in &workflow.steps {
            let descriptor = self.descriptor_for_step(&step.id)?.spec();
            if descriptor.owner == WorkflowStepOwner::Applications {
                if !is_exact_application_answer_descriptor(descriptor) {
                    return Err(format!(
                        "Application Workflow step {:?} is not the supported application.answer port",
                        step.id
                    ));
                }
                answer_step_ids.insert(step.id.clone());
                continue;
            }
            if step.kind == WorkflowStepKind::Output {
                if !is_exact_application_final_output_descriptor(descriptor) {
                    return Err(format!(
                        "Application Workflow Output step {:?} is not the Workflow-owned workflow.output port",
                        step.id
                    ));
                }
                final_output_step_ids.push(step.id.clone());
            }
        }
        let [final_output_step_id] = final_output_step_ids.as_slice() else {
            return Err(
                "Application Workflow requires exactly one Workflow-owned workflow.output step"
                    .into(),
            );
        };
        Ok(WorkflowApplicationOutputSteps {
            final_output_step_id: final_output_step_id.clone(),
            answer_step_ids,
        })
    }

    pub(crate) fn failure_contract(
        &self,
        step_id: &str,
    ) -> Result<&super::WorkflowStepFailureContract, String> {
        Ok(&self.descriptor_for_step(step_id)?.spec().failure)
    }

    fn descriptor_for_step(
        &self,
        step_id: &str,
    ) -> Result<&super::WorkflowStepDescriptorRevision, String> {
        let binding = self
            .descriptor_bindings
            .resolve(step_id)
            .ok_or_else(|| format!("Workflow step {step_id:?} lost its descriptor binding"))?;
        let descriptor = self
            .descriptor_registry
            .resolve(&binding.descriptor_id, &binding.descriptor_revision)
            .ok_or_else(|| format!("Workflow step {step_id:?} lost its descriptor revision"))?;
        if descriptor.semantic_digest() != &binding.semantic_digest {
            return Err(format!(
                "Workflow step {step_id:?} descriptor semantic authority drifted"
            ));
        }
        Ok(descriptor)
    }

    pub(crate) fn has_default_output_fallback(&self) -> bool {
        self.descriptor_bindings.bindings().iter().any(|binding| {
            self.descriptor_registry
                .resolve(&binding.descriptor_id, &binding.descriptor_revision)
                .is_some_and(|descriptor| {
                    descriptor.spec().failure.fallback == WorkflowStepFallbackMode::DefaultOutput
                })
        })
    }

    pub(crate) fn default_output_contract(
        &self,
        step_id: &str,
    ) -> Result<Option<WorkflowStepDefaultOutputContract>, String> {
        let descriptor = self.descriptor_for_step(step_id)?;
        let spec = descriptor.spec();
        if spec.failure.fallback != WorkflowStepFallbackMode::DefaultOutput {
            return Ok(None);
        }
        let [output_port] = spec.output_ports.as_slice() else {
            return Err(format!(
                "Workflow default-output step {step_id:?} must expose exactly one output port"
            ));
        };
        let contract = WorkflowStepDefaultOutputContract {
            output_port: output_port.clone(),
        };
        contract.validate()?;
        Ok(Some(contract))
    }

    pub const fn variable_contract(&self) -> &WorkflowVariableContract {
        &self.variable_contract
    }

    pub const fn variable_defaults(&self) -> Option<&WorkflowVariableDefaults> {
        self.variable_defaults.as_ref()
    }

    pub const fn composite_regions(&self) -> Option<&WorkflowCompositeRegions> {
        self.composite_regions.as_ref()
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub(crate) fn validate_plan_bindings(&self, plan: &WorkflowPlan) -> Result<(), String> {
        if plan.semantic_contract_set_digest.as_ref() != Some(&self.digest)
            || plan.variable_contract_digest.as_ref() != Some(self.variable_contract.digest())
            || plan.composite_regions_digest.as_ref()
                != self
                    .composite_regions
                    .as_ref()
                    .map(WorkflowCompositeRegions::digest)
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
            let expected_failure = self.failure_contract(&step.id)?;
            let expected_default_output = self.default_output_contract(&step.id)?;
            let descriptor = self.descriptor_for_step(&step.id)?;
            if expected_default_output.is_some()
                && step.policy_digest.as_ref() != descriptor.spec().default_policy_digest.as_ref()
            {
                return Err(format!(
                    "Workflow plan step {:?} default policy authority drifted",
                    step.id
                ));
            }
            match plan.schema.as_str() {
                super::WORKFLOW_PLAN_SCHEMA_V2
                    if step.failure.is_none() && step.default_output.is_none() => {}
                super::WORKFLOW_PLAN_SCHEMA_V3
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output.is_none() => {}
                super::WORKFLOW_PLAN_SCHEMA_V4
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output == expected_default_output => {}
                super::WORKFLOW_PLAN_SCHEMA_V5
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output == expected_default_output => {}
                _ => {
                    return Err(format!(
                        "Workflow plan step {:?} failure semantics drifted",
                        step.id
                    ))
                }
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
        if let Some(regions) = &self.composite_regions {
            values.push(WorkflowRevisionSemanticContractRef {
                kind: WorkflowRevisionSemanticContractKind::CompositeRegions,
                schema: WORKFLOW_COMPOSITE_REGIONS_SCHEMA,
                canonical_acl: regions.canonical_acl(),
                digest: regions.digest(),
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

    fn validate_composite_region_material(
        &self,
        workflow: &WorkflowSpec,
        require_complete: bool,
    ) -> Result<(), String> {
        let steps = workflow
            .steps
            .iter()
            .map(|step| (step.id.as_str(), step))
            .collect::<BTreeMap<_, _>>();
        let mut expected = BTreeMap::new();
        for binding in self.descriptor_bindings.bindings() {
            let descriptor = self
                .descriptor_registry
                .resolve(&binding.descriptor_id, &binding.descriptor_revision)
                .ok_or_else(|| {
                    format!(
                        "Workflow composite region step {:?} lost its descriptor",
                        binding.step_id
                    )
                })?;
            if descriptor.spec().execution_class == WorkflowStepExecutionClass::CompositeRegion {
                expected.insert(binding.step_id.as_str(), descriptor.spec());
            }
        }
        match (expected.is_empty(), self.composite_regions.as_ref()) {
            (true, None) => return Ok(()),
            (true, Some(_)) => {
                return Err(
                    "Workflow composite region material exists without a composite descriptor"
                        .into(),
                )
            }
            (false, None) if require_complete => {
                return Err(
                    "Workflow composite descriptors require immutable region material".into(),
                )
            }
            (false, None) => return Ok(()),
            (false, Some(_)) => {}
        }
        let regions = self.composite_regions.as_ref().ok_or_else(|| {
            "Workflow composite descriptors require immutable region material".to_owned()
        })?;
        if regions.spec().regions.len() != expected.len() {
            return Err(
                "Workflow composite regions must exactly cover composite descriptors".into(),
            );
        }
        for (step_id, descriptor) in expected {
            let policy = regions.resolve(step_id).ok_or_else(|| {
                format!("Workflow composite step {step_id:?} has no region policy")
            })?;
            if policy.semantic_profile() != descriptor.semantic_profile {
                return Err(format!(
                    "Workflow composite step {step_id:?} policy does not match its semantic profile"
                ));
            }
            let step = steps
                .get(step_id)
                .ok_or_else(|| format!("Workflow composite step {step_id:?} disappeared"))?;
            let capability = step.capability.as_ref().ok_or_else(|| {
                format!("Workflow composite step {step_id:?} has no child Workflow revision")
            })?;
            if !is_exact_child_workflow_revision(capability) {
                return Err(format!(
                    "Workflow composite step {step_id:?} must bind one exact workflow.run revision"
                ));
            }
        }
        Ok(())
    }
}

fn validate_connector_retry_authority(
    step: &super::WorkflowStepSpec,
    descriptor: &super::WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    let connector = step
        .capability
        .as_ref()
        .is_some_and(|capability| capability.capability_type == CapabilityType::ConnectorRevision);
    if !connector {
        return Ok(());
    }
    if descriptor.owner != WorkflowStepOwner::Connectors
        || descriptor.semantic_profile != "connector.http"
        || descriptor.failure.retry_classification
            != WorkflowStepRetryClassification::OwnerClassified
    {
        return Err(format!(
            "Workflow Connector step {:?} lacks Connectors-owned retry classification",
            step.id
        ));
    }
    Ok(())
}

fn validate_default_output_authority(
    step: &super::WorkflowStepSpec,
    descriptor: &super::WorkflowStepDescriptorSpec,
) -> Result<(), String> {
    if descriptor.failure.fallback != WorkflowStepFallbackMode::DefaultOutput {
        return Ok(());
    }
    if step.kind != WorkflowStepKind::Execution
        || descriptor.owner != WorkflowStepOwner::Executions
        || descriptor.execution_class != WorkflowStepExecutionClass::OwningApplicationPort
        || step.policy_digest.as_ref() != descriptor.default_policy_digest.as_ref()
        || descriptor.failure.error_output.is_some()
        || descriptor.failure.retry_classification
            != WorkflowStepRetryClassification::OwnerClassified
        || !step.capability.as_ref().is_some_and(|capability| {
            capability.capability_type == CapabilityType::ExecutionTemplate
        })
    {
        return Err(format!(
            "Workflow step {:?} default-output fallback requires the Executions-owned finite Execution port and its exact descriptor policy",
            step.id
        ));
    }
    let [port] = descriptor.output_ports.as_slice() else {
        return Err(format!(
            "Workflow default-output step {:?} must expose exactly one output port",
            step.id
        ));
    };
    if port.cardinality != WorkflowStepPortCardinality::Single || !port.required || port.dynamic {
        return Err(format!(
            "Workflow default-output step {:?} must expose one required static output port",
            step.id
        ));
    }
    Ok(())
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
        ) && !(*binding == &WorkflowStepBindingKind::ReleaseReference
            && is_exact_application_answer_descriptor(descriptor))
    }) {
        return Err(format!(
            "Workflow step {:?} descriptor requires unsupported {} binding",
            step.id,
            unsupported.as_str()
        ));
    }
    Ok(())
}

fn is_exact_application_answer_descriptor(descriptor: &super::WorkflowStepDescriptorSpec) -> bool {
    descriptor.id == "application.answer"
        && descriptor.semantic_profile == "application.answer"
        && descriptor.owner == WorkflowStepOwner::Applications
        && descriptor.kind == Some(WorkflowStepKind::Output)
        && descriptor.execution_class == WorkflowStepExecutionClass::OwningApplicationPort
        && descriptor.required_bindings == [WorkflowStepBindingKind::ReleaseReference]
        && descriptor.allowed_capability_types.is_empty()
        && descriptor.default_policy_digest.is_none()
        && descriptor.failure.error_output.is_none()
        && descriptor.failure.retry_classification == WorkflowStepRetryClassification::NotRetryable
        && descriptor.failure.fallback == WorkflowStepFallbackMode::Unsupported
        && !descriptor.failure.failure_branch
}

fn is_exact_application_final_output_descriptor(
    descriptor: &super::WorkflowStepDescriptorSpec,
) -> bool {
    descriptor.id == "workflow.output"
        && descriptor.semantic_profile == "workflow.output"
        && descriptor.owner == WorkflowStepOwner::Workflow
        && descriptor.kind == Some(WorkflowStepKind::Output)
        && descriptor.execution_class == WorkflowStepExecutionClass::WorkflowLocal
        && descriptor
            .required_bindings
            .iter()
            .all(|binding| *binding == WorkflowStepBindingKind::PlacementPolicy)
        && descriptor.allowed_capability_types.is_empty()
        && descriptor.default_policy_digest.is_none()
        && descriptor.failure.error_output.is_none()
        && descriptor.failure.retry_classification == WorkflowStepRetryClassification::NotRetryable
        && descriptor.failure.fallback == WorkflowStepFallbackMode::Unsupported
        && !descriptor.failure.failure_branch
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
    #[serde(skip_serializing_if = "Option::is_none")]
    composite_regions_digest: Option<&'a str>,
}

fn digest_contract_set(
    bindings: &WorkflowStepDescriptorBindings,
    variables: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
    composite_regions: Option<&WorkflowCompositeRegions>,
) -> Result<Sha256Digest, String> {
    let encoded = serde_json::to_vec(&SemanticContractDigestInput {
        descriptor_bindings_digest: bindings.digest().as_str(),
        variable_contract_digest: variables.digest().as_str(),
        variable_defaults_digest: defaults.map(|value| value.digest().as_str()),
        composite_regions_digest: composite_regions.map(|value| value.digest().as_str()),
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

#[cfg(test)]
mod connector_retry_authority_tests {
    use super::*;
    use crate::modules::workflow::domain::{
        CapabilityOwner, CapabilityReference, WorkflowStepDescriptorAdmission,
        WorkflowStepFailureContract, WorkflowStepFallbackMode, WorkflowStepKind,
        WorkflowStepPresentationSpec,
    };
    use uuid::Uuid;

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn step() -> super::super::WorkflowStepSpec {
        super::super::WorkflowStepSpec {
            id: "invoke".into(),
            label: "Invoke".into(),
            kind: WorkflowStepKind::Service,
            configuration_digest: digest('a'),
            input_schema_digest: digest('b'),
            output_schema_digest: digest('c'),
            policy_digest: Some(digest('d')),
            capability: Some(CapabilityReference {
                owner: CapabilityOwner::Connectors,
                capability_type: CapabilityType::ConnectorRevision,
                resource_id: Uuid::now_v7(),
                revision: Uuid::now_v7().to_string(),
                digest: digest('e'),
                capability: "connector.http".into(),
            }),
        }
    }

    fn descriptor() -> super::super::WorkflowStepDescriptorSpec {
        super::super::WorkflowStepDescriptorSpec {
            id: "connector.http".into(),
            revision: "1.0.0".into(),
            owner: WorkflowStepOwner::Connectors,
            kind: Some(WorkflowStepKind::Service),
            semantic_profile: "connector.http".into(),
            execution_class: WorkflowStepExecutionClass::OwningApplicationPort,
            input_ports: Vec::new(),
            output_ports: Vec::new(),
            configuration_schema_digest: digest('a'),
            default_policy_digest: None,
            required_bindings: vec![WorkflowStepBindingKind::CapabilityReference],
            allowed_capability_types: vec![CapabilityType::ConnectorRevision],
            failure: WorkflowStepFailureContract {
                error_output: None,
                retry_classification: WorkflowStepRetryClassification::OwnerClassified,
                fallback: WorkflowStepFallbackMode::Unsupported,
                failure_branch: false,
            },
            minimum_compiler_schema_version: 2,
            maximum_compiler_schema_version: 2,
            admission: WorkflowStepDescriptorAdmission::Admitted,
            unavailable_reason: None,
            presentation: WorkflowStepPresentationSpec {
                label: "HTTP Request".into(),
                summary: "Calls one exact Connector revision".into(),
                icon_key: "connector.http".into(),
            },
        }
    }

    #[test]
    fn connector_retry_classification_stays_with_the_connectors_owner() {
        let step = step();
        let descriptor = descriptor();
        validate_connector_retry_authority(&step, &descriptor).expect("connector authority");

        let mut drifted = descriptor.clone();
        drifted.owner = WorkflowStepOwner::Workflow;
        assert!(validate_connector_retry_authority(&step, &drifted).is_err());
        drifted = descriptor.clone();
        drifted.semantic_profile = "service.http".into();
        assert!(validate_connector_retry_authority(&step, &drifted).is_err());
        drifted = descriptor;
        drifted.failure.retry_classification = WorkflowStepRetryClassification::FlowRetryable;
        assert!(validate_connector_retry_authority(&step, &drifted).is_err());
    }
}
