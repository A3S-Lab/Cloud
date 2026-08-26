use super::workflow_composite_regions::is_exact_child_workflow_revision;
use super::{
    has_application_answer_failure_route, has_application_variable_failure_route,
    has_branch_failure_route, has_composite_failure_route, has_workflow_output_failure_route,
    validate_descriptor_failure_routes, WorkflowCompositeRegions, WorkflowPlan, WorkflowSpec,
    WorkflowStepBindingKind, WorkflowStepDefaultOutputContract, WorkflowStepDescriptorBindings,
    WorkflowStepDescriptorRegistry, WorkflowStepExecutionClass, WorkflowStepFallbackMode,
    WorkflowStepKind, WorkflowStepOwner, WorkflowVariableContract, WorkflowVariableDefaults,
    WORKFLOW_COMPOSITE_REGIONS_SCHEMA, WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA,
    WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA, WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
    WORKFLOW_VARIABLE_CONTRACT_SCHEMA, WORKFLOW_VARIABLE_DEFAULTS_SCHEMA,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use std::collections::{BTreeMap, BTreeSet};

#[path = "workflow_revision_semantic_contracts/validation.rs"]
mod validation;

use validation::{
    descriptor_has_runtime_dispatch, digest_contract_set, is_exact_agent_release_capability,
    is_exact_application_answer_descriptor, is_exact_application_final_output_descriptor,
    is_exact_application_variable_descriptor, is_exact_workflow_output_descriptor,
    validate_capability_binding, validate_connector_retry_authority, validate_default_material,
    validate_default_output_authority, validate_supported_bindings, validate_variable_read_ports,
};

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
    pub variable_step_ids: BTreeSet<String>,
    pub variable_assignment_step_ids: BTreeSet<String>,
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
        let mut application_variable_steps = BTreeSet::new();
        let mut application_answer_steps = BTreeSet::new();
        let mut workflow_output_steps = BTreeSet::new();
        let mut composite_steps = BTreeSet::new();
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
            if is_exact_application_variable_descriptor(descriptor.spec()) {
                application_variable_steps.insert(step.id.as_str());
            }
            if is_exact_application_answer_descriptor(descriptor.spec()) {
                application_answer_steps.insert(step.id.as_str());
            }
            if is_exact_workflow_output_descriptor(descriptor.spec()) {
                workflow_output_steps.insert(step.id.as_str());
            }
            if descriptor.spec().owner == WorkflowStepOwner::Workflow
                && descriptor.spec().execution_class == WorkflowStepExecutionClass::CompositeRegion
            {
                composite_steps.insert(step.id.as_str());
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
        validate_descriptor_failure_routes(
            workflow,
            &failures_by_step,
            &application_variable_steps,
            &application_answer_steps,
            &workflow_output_steps,
            &composite_steps,
        )?;
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
        let mut variable_port_step_ids = BTreeSet::new();
        for step in &workflow.steps {
            let descriptor = self.descriptor_for_step(&step.id)?.spec();
            if descriptor.owner == WorkflowStepOwner::Applications {
                if is_exact_application_answer_descriptor(descriptor) {
                    answer_step_ids.insert(step.id.clone());
                } else if is_exact_application_variable_descriptor(descriptor) {
                    variable_port_step_ids.insert(step.id.clone());
                } else {
                    return Err(format!(
                        "Application Workflow step {:?} is not a supported exact Applications port",
                        step.id
                    ));
                }
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
        let application_variables = self
            .variable_contract
            .spec()
            .declarations
            .iter()
            .filter(|declaration| declaration.scope == super::WorkflowVariableScope::Application)
            .map(|declaration| declaration.name.as_str())
            .collect::<BTreeSet<_>>();
        let mut variable_step_ids = BTreeSet::new();
        let mut variable_assignment_step_ids = BTreeSet::new();
        for read in &self.variable_contract.spec().reads {
            if application_variables.contains(read.variable.as_str()) {
                variable_step_ids.insert(read.consumer_step_id.clone());
            }
        }
        for assignment in &self.variable_contract.spec().assignments {
            if application_variables.contains(assignment.source_variable.as_str())
                || assignment
                    .expected_revision_variable
                    .as_deref()
                    .is_some_and(|name| application_variables.contains(name))
                || assignment
                    .idempotency_key_variable
                    .as_deref()
                    .is_some_and(|name| application_variables.contains(name))
            {
                variable_step_ids.insert(assignment.writer_step_id.clone());
            }
            if application_variables.contains(assignment.target_variable.as_str()) {
                if !variable_port_step_ids.contains(&assignment.writer_step_id) {
                    return Err(format!(
                        "Application variable assignment {:?} does not use the exact application.conversation-variable-assign port",
                        assignment.id
                    ));
                }
                variable_step_ids.insert(assignment.writer_step_id.clone());
                variable_assignment_step_ids.insert(assignment.writer_step_id.clone());
            }
        }
        if variable_port_step_ids != variable_assignment_step_ids {
            return Err(
                "Every application.conversation-variable-assign step must own at least one exact Application variable assignment"
                    .into(),
            );
        }
        let application_ports = answer_step_ids
            .union(&variable_port_step_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        if variable_step_ids
            .iter()
            .any(|step_id| !application_ports.contains(step_id))
        {
            return Err(
                "Application variable access requires an exact descriptor-bound Applications port"
                    .into(),
            );
        }
        Ok(WorkflowApplicationOutputSteps {
            final_output_step_id: final_output_step_id.clone(),
            answer_step_ids,
            variable_step_ids,
            variable_assignment_step_ids,
        })
    }

    pub(crate) fn failure_contract(
        &self,
        step_id: &str,
    ) -> Result<&super::WorkflowStepFailureContract, String> {
        Ok(&self.descriptor_for_step(step_id)?.spec().failure)
    }

    pub(crate) fn descriptor_spec(
        &self,
        step_id: &str,
    ) -> Result<&super::WorkflowStepDescriptorSpec, String> {
        Ok(self.descriptor_for_step(step_id)?.spec())
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

    pub(crate) fn has_application_variable_failure_route(&self, workflow: &WorkflowSpec) -> bool {
        let steps = workflow
            .steps
            .iter()
            .filter(|step| {
                step.kind == WorkflowStepKind::Service
                    && step.capability.is_none()
                    && self
                        .descriptor_bindings
                        .resolve(&step.id)
                        .is_some_and(|binding| {
                            binding.descriptor_id == "application.conversation-variable-assign"
                        })
            })
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        has_application_variable_failure_route(workflow, &steps)
    }

    pub(crate) fn has_application_answer_failure_route(&self, workflow: &WorkflowSpec) -> bool {
        let steps = workflow
            .steps
            .iter()
            .filter(|step| {
                step.kind == WorkflowStepKind::Output
                    && step.capability.is_none()
                    && self
                        .descriptor_bindings
                        .resolve(&step.id)
                        .is_some_and(|binding| binding.descriptor_id == "application.answer")
            })
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        has_application_answer_failure_route(workflow, &steps)
    }

    pub(crate) fn has_workflow_output_failure_route(&self, workflow: &WorkflowSpec) -> bool {
        let steps = workflow
            .steps
            .iter()
            .filter(|step| {
                step.kind == WorkflowStepKind::Output
                    && step.capability.is_none()
                    && self
                        .descriptor_bindings
                        .resolve(&step.id)
                        .is_some_and(|binding| binding.descriptor_id == "workflow.output")
            })
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        has_workflow_output_failure_route(workflow, &steps)
    }

    pub(crate) fn has_branch_failure_route(&self, workflow: &WorkflowSpec) -> bool {
        let failures = workflow
            .steps
            .iter()
            .filter(|step| step.kind == WorkflowStepKind::Branch)
            .filter_map(|step| {
                self.descriptor_for_step(&step.id)
                    .ok()
                    .map(|descriptor| (step.id.as_str(), &descriptor.spec().failure))
            })
            .collect::<BTreeMap<_, _>>();
        has_branch_failure_route(workflow, &failures)
    }

    pub(crate) fn has_composite_failure_route(&self, workflow: &WorkflowSpec) -> bool {
        let steps = workflow
            .steps
            .iter()
            .filter(|step| {
                step.kind == WorkflowStepKind::Subworkflow
                    && self.descriptor_for_step(&step.id).is_ok_and(|descriptor| {
                        descriptor.spec().owner == WorkflowStepOwner::Workflow
                            && descriptor.spec().execution_class
                                == WorkflowStepExecutionClass::CompositeRegion
                    })
            })
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        has_composite_failure_route(workflow, &steps)
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
        let workflow = plan.workflow_spec()?;
        if self.has_composite_failure_route(&workflow)
            && plan.schema != super::WORKFLOW_PLAN_SCHEMA_V11
        {
            return Err(
                "Workflow descriptor-bound composite failure routes require Plan v11".into(),
            );
        }
        if self.has_branch_failure_route(&workflow)
            && !matches!(
                plan.schema.as_str(),
                super::WORKFLOW_PLAN_SCHEMA_V10 | super::WORKFLOW_PLAN_SCHEMA_V11
            )
        {
            return Err("Workflow descriptor-bound Branch failure routes require Plan v10".into());
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
                super::WORKFLOW_PLAN_SCHEMA_V6
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output == expected_default_output => {}
                super::WORKFLOW_PLAN_SCHEMA_V7
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output == expected_default_output => {}
                super::WORKFLOW_PLAN_SCHEMA_V8
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output == expected_default_output => {}
                super::WORKFLOW_PLAN_SCHEMA_V9
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output == expected_default_output => {}
                super::WORKFLOW_PLAN_SCHEMA_V10
                    if step.failure.as_ref() == Some(expected_failure)
                        && step.default_output == expected_default_output => {}
                super::WORKFLOW_PLAN_SCHEMA_V11
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

    pub(crate) fn validate_runtime_dispatch_support(
        &self,
        workflow: &WorkflowSpec,
    ) -> Result<(), String> {
        for step in &workflow.steps {
            let descriptor = self.descriptor_for_step(&step.id)?.spec();
            if descriptor_has_runtime_dispatch(descriptor) {
                if step.kind == WorkflowStepKind::Agent
                    && !is_exact_agent_release_capability(step.capability.as_ref())
                {
                    return Err(format!(
                        "Workflow Agent step {:?} must bind one exact agent.execute release",
                        step.id
                    ));
                }
                continue;
            }
            return Err(format!(
                "Workflow step {:?} descriptor {:?}@{:?} has no admitted Cloud runtime dispatch port",
                step.id, descriptor.id, descriptor.revision
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "workflow_revision_semantic_contracts_connector_tests.rs"]
mod connector_retry_authority_tests;
