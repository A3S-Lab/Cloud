use super::workflow_revision::application_workflow_revision_evidence;
use crate::modules::applications::application::{
    ApplicationPresetTarget, ApplicationPresetWorkflowRequest, ApplicationPresetWorkflowResult,
    IApplicationPresetWorkflowPort,
};
use crate::modules::applications::domain::ApplicationExperience;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::Sha256Digest;
use crate::modules::workflow::application::{
    IWorkflowDefinitionPublicationPort, WorkflowDefinitionPublicationRequest, WorkflowPayloadAcl,
    WorkflowSemanticContractAcls,
};
use crate::modules::workflow::domain::{
    CapabilityOwner, CapabilityReference, CapabilityType, WorkflowContract, WorkflowDataSchema,
    WorkflowDataType, WorkflowEdgeSpec, WorkflowPayload, WorkflowPayloadContent,
    WorkflowRevisionSemanticContracts, WorkflowSpec, WorkflowStepBindingKind,
    WorkflowStepConfiguration, WorkflowStepDescriptorAdmission, WorkflowStepDescriptorBinding,
    WorkflowStepDescriptorBindings, WorkflowStepDescriptorBindingsSpec,
    WorkflowStepDescriptorRegistry, WorkflowStepDescriptorRegistrySpec, WorkflowStepDescriptorSpec,
    WorkflowStepExecutionClass, WorkflowStepFailureContract, WorkflowStepFallbackMode,
    WorkflowStepKind, WorkflowStepOwner, WorkflowStepPort, WorkflowStepPortCardinality,
    WorkflowStepPresentationSpec, WorkflowStepRetryClassification, WorkflowStepSpec,
    WorkflowVariableContract, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
    WorkflowVariableMutationMode, WorkflowVariableRead, WorkflowVariableReadMode,
    WorkflowVariableScope, WorkflowVariableStorageClass,
    WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
};
use async_trait::async_trait;
use std::sync::Arc;

const PRESET_CONTRACT_REVISION: &str = "1.0.0";
const INPUT_STEP_ID: &str = "input";
const TARGET_STEP_ID: &str = "invoke";
const OUTPUT_STEP_ID: &str = "output";
const INPUT_DESCRIPTOR_ID: &str = "workflow.user-input";
const OUTPUT_DESCRIPTOR_ID: &str = "workflow.output";

/// Production adapter that deterministically compiles an Applications preset
/// and delegates persistence to Workflow's sole definition-publication port.
pub struct WorkflowApplicationPresetCompiler {
    publications: Arc<dyn IWorkflowDefinitionPublicationPort>,
}

impl WorkflowApplicationPresetCompiler {
    pub fn new(publications: Arc<dyn IWorkflowDefinitionPublicationPort>) -> Self {
        Self { publications }
    }
}

#[async_trait]
impl IApplicationPresetWorkflowPort for WorkflowApplicationPresetCompiler {
    async fn compile_and_publish(
        &self,
        request: &ApplicationPresetWorkflowRequest,
    ) -> ApplicationResult<ApplicationPresetWorkflowResult> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let compiled = CompiledPresetWorkflow::compile(request)?;
        let definition_id = request.workflow_definition_id();
        let revision_id = request.workflow_revision_id();
        let result = self
            .publications
            .publish(WorkflowDefinitionPublicationRequest {
                organization_id: request.organization_id,
                project_id: request.project_id,
                definition_id,
                revision_id,
                definition_acl: compiled.contract.canonical_acl().to_owned(),
                payloads: compiled
                    .payloads
                    .iter()
                    .map(|payload| WorkflowPayloadAcl {
                        kind: payload.kind(),
                        acl: payload.canonical_acl().to_owned(),
                    })
                    .collect(),
                semantic_contracts: Some(WorkflowSemanticContractAcls {
                    descriptor_bindings_acl: compiled
                        .semantic_contracts
                        .descriptor_bindings()
                        .canonical_acl()
                        .to_owned(),
                    descriptor_registry_acl: compiled
                        .semantic_contracts
                        .descriptor_registry()
                        .canonical_acl()
                        .to_owned(),
                    variable_contract_acl: compiled
                        .semantic_contracts
                        .variable_contract()
                        .canonical_acl()
                        .to_owned(),
                    variable_defaults_acl: None,
                    composite_regions_acl: None,
                }),
                actor_principal_id: request.actor_principal_id,
                idempotency_scope: format!(
                    "organizations/{}/projects/{}/applications/{}/releases/{}/preset-workflow",
                    request.organization_id,
                    request.project_id,
                    request.application_id,
                    request.application_release_number
                ),
                idempotency_key: request.idempotency_key.clone(),
                request_id: request.request_id,
            })
            .await?;
        let revision = &result.record.revision;
        if result.record.definition.id != definition_id
            || revision.id != revision_id
            || revision.contract != compiled.contract
            || revision.payloads != compiled.payloads
            || revision.semantic_contracts.as_ref() != Some(&compiled.semantic_contracts)
        {
            return Err(ApplicationError::Conflict(
                "Application preset Workflow publication drifted from its deterministic compilation"
                    .into(),
            ));
        }
        let evidence = application_workflow_revision_evidence(revision)?;
        if evidence.binding.workflow_definition_id != definition_id
            || evidence.binding.workflow_revision_id != revision_id
        {
            return Err(ApplicationError::Conflict(
                "Application preset Workflow evidence drifted from its stable identity".into(),
            ));
        }
        Ok(ApplicationPresetWorkflowResult {
            evidence,
            replayed: result.replayed,
        })
    }
}

#[derive(Clone)]
struct CompiledPresetWorkflow {
    contract: WorkflowContract,
    payloads: Vec<WorkflowPayload>,
    semantic_contracts: WorkflowRevisionSemanticContracts,
}

impl CompiledPresetWorkflow {
    fn compile(request: &ApplicationPresetWorkflowRequest) -> ApplicationResult<Self> {
        let target = TargetSemantics::from_request(request)?;
        let schema =
            WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
                value_type: WorkflowDataType::Object,
                fields: Vec::new(),
            }))
            .map_err(ApplicationError::Invalid)?;
        let input_configuration = configuration(WorkflowStepKind::Input)?;
        let target_configuration = configuration(target.kind)?;
        let output_configuration = configuration(WorkflowStepKind::Output)?;
        let schema_digest = schema.digest().clone();
        let workflow = WorkflowSpec {
            name: format!(
                "Application {} release {} {} preset",
                request.application_id,
                request.application_release_number,
                request.experience.as_str()
            ),
            description: format!(
                "Deterministic {} wrapper generated by Applications",
                request.experience.as_str()
            ),
            steps: vec![
                step(
                    INPUT_STEP_ID,
                    "Input",
                    WorkflowStepKind::Input,
                    input_configuration.digest().clone(),
                    schema_digest.clone(),
                    None,
                ),
                step(
                    TARGET_STEP_ID,
                    target.label,
                    target.kind,
                    target_configuration.digest().clone(),
                    schema_digest.clone(),
                    Some(target.capability.clone()),
                ),
                step(
                    OUTPUT_STEP_ID,
                    "Output",
                    WorkflowStepKind::Output,
                    output_configuration.digest().clone(),
                    schema_digest.clone(),
                    None,
                ),
            ],
            edges: vec![
                WorkflowEdgeSpec {
                    id: "input-invoke".into(),
                    source: INPUT_STEP_ID.into(),
                    target: TARGET_STEP_ID.into(),
                    source_handle: None,
                },
                WorkflowEdgeSpec {
                    id: "invoke-output".into(),
                    source: TARGET_STEP_ID.into(),
                    target: OUTPUT_STEP_ID.into(),
                    source_handle: None,
                },
            ],
        };
        let contract =
            WorkflowContract::from_spec(workflow.clone()).map_err(ApplicationError::Invalid)?;
        let registry_id = request.experience.registry_id();
        let registry =
            WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
                id: registry_id.into(),
                revision: PRESET_CONTRACT_REVISION.into(),
                compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
                descriptors: vec![
                    local_descriptor(
                        INPUT_DESCRIPTOR_ID,
                        WorkflowStepKind::Input,
                        Vec::new(),
                        vec![port("request")],
                        input_configuration.digest().clone(),
                        "Application input",
                    ),
                    target.descriptor(target_configuration.digest().clone()),
                    local_descriptor(
                        OUTPUT_DESCRIPTOR_ID,
                        WorkflowStepKind::Output,
                        vec![port("result")],
                        vec![port("result")],
                        output_configuration.digest().clone(),
                        "Application output",
                    ),
                ],
            })
            .map_err(ApplicationError::Invalid)?;
        let bindings = descriptor_bindings(registry_id, &registry, target.descriptor_id)?;
        let variables = variable_contract(registry_id, schema_digest)?;
        let semantic_contracts =
            WorkflowRevisionSemanticContracts::create(&workflow, bindings, registry, variables)
                .map_err(ApplicationError::Invalid)?;
        let mut payloads = vec![
            schema,
            input_configuration,
            target_configuration,
            output_configuration,
        ];
        payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
        Ok(Self {
            contract,
            payloads,
            semantic_contracts,
        })
    }
}

struct TargetSemantics {
    kind: WorkflowStepKind,
    owner: WorkflowStepOwner,
    capability_type: CapabilityType,
    descriptor_id: &'static str,
    semantic_profile: &'static str,
    label: &'static str,
    capability: CapabilityReference,
}

impl TargetSemantics {
    fn from_request(request: &ApplicationPresetWorkflowRequest) -> ApplicationResult<Self> {
        let (kind, owner, capability_type, descriptor_id, semantic_profile, label, capability) =
            match (request.experience, &request.target) {
                (
                    ApplicationExperience::Chatbot | ApplicationExperience::TextGenerator,
                    ApplicationPresetTarget::ModelRevision(target),
                ) => (
                    WorkflowStepKind::Model,
                    WorkflowStepOwner::Inference,
                    CapabilityType::ModelRevision,
                    "model.llm",
                    "model.llm",
                    "Model",
                    CapabilityReference {
                        owner: CapabilityOwner::Inference,
                        capability_type: CapabilityType::ModelRevision,
                        resource_id: target.model_id,
                        revision: target.revision.clone(),
                        digest: target.digest.clone(),
                        capability: target.capability.clone(),
                    },
                ),
                (
                    ApplicationExperience::ClassicAgent,
                    ApplicationPresetTarget::AgentRelease(target),
                ) => agent_target(target, "agent.classic", "Classic Agent"),
                (
                    ApplicationExperience::NewAgent,
                    ApplicationPresetTarget::AgentRelease(target),
                ) => agent_target(target, "agent.release", "New Agent"),
                _ => {
                    return Err(ApplicationError::Invalid(
                        "Application preset experience and target do not match".into(),
                    ))
                }
            };
        capability.validate().map_err(ApplicationError::Invalid)?;
        Ok(Self {
            kind,
            owner,
            capability_type,
            descriptor_id,
            semantic_profile,
            label,
            capability,
        })
    }

    fn descriptor(&self, configuration_digest: Sha256Digest) -> WorkflowStepDescriptorSpec {
        WorkflowStepDescriptorSpec {
            id: self.descriptor_id.into(),
            revision: PRESET_CONTRACT_REVISION.into(),
            owner: self.owner,
            kind: Some(self.kind),
            semantic_profile: self.semantic_profile.into(),
            execution_class: WorkflowStepExecutionClass::OwningApplicationPort,
            input_ports: vec![port("request")],
            output_ports: vec![port("result")],
            configuration_schema_digest: configuration_digest,
            default_policy_digest: None,
            required_bindings: vec![WorkflowStepBindingKind::CapabilityReference],
            allowed_capability_types: vec![self.capability_type],
            failure: WorkflowStepFailureContract {
                error_output: None,
                retry_classification: WorkflowStepRetryClassification::OwnerClassified,
                fallback: WorkflowStepFallbackMode::Unsupported,
                failure_branch: false,
            },
            minimum_compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            maximum_compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
            admission: WorkflowStepDescriptorAdmission::Admitted,
            unavailable_reason: None,
            presentation: WorkflowStepPresentationSpec {
                label: self.label.into(),
                summary: format!("Calls one exact {} capability", self.semantic_profile),
                icon_key: self.semantic_profile.into(),
            },
        }
    }
}

fn agent_target(
    target: &crate::modules::applications::application::ApplicationPresetAgentRelease,
    descriptor_id: &'static str,
    label: &'static str,
) -> (
    WorkflowStepKind,
    WorkflowStepOwner,
    CapabilityType,
    &'static str,
    &'static str,
    &'static str,
    CapabilityReference,
) {
    (
        WorkflowStepKind::Agent,
        WorkflowStepOwner::Agents,
        CapabilityType::AgentRelease,
        descriptor_id,
        descriptor_id,
        label,
        CapabilityReference {
            owner: CapabilityOwner::Assets,
            capability_type: CapabilityType::AgentRelease,
            resource_id: target.asset_id.as_uuid(),
            revision: target.asset_release_id.to_string(),
            digest: target.digest.clone(),
            capability: target.capability.clone(),
        },
    )
}

fn configuration(kind: WorkflowStepKind) -> ApplicationResult<WorkflowPayload> {
    WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
        WorkflowStepConfiguration::empty(kind),
    ))
    .map_err(ApplicationError::Invalid)
}

fn step(
    id: &str,
    label: &str,
    kind: WorkflowStepKind,
    configuration_digest: Sha256Digest,
    schema_digest: Sha256Digest,
    capability: Option<CapabilityReference>,
) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: id.into(),
        label: label.into(),
        kind,
        configuration_digest,
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest,
        policy_digest: None,
        capability,
    }
}

fn local_descriptor(
    id: &str,
    kind: WorkflowStepKind,
    input_ports: Vec<WorkflowStepPort>,
    output_ports: Vec<WorkflowStepPort>,
    configuration_digest: Sha256Digest,
    label: &str,
) -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: id.into(),
        revision: PRESET_CONTRACT_REVISION.into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(kind),
        semantic_profile: id.into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports,
        output_ports,
        configuration_schema_digest: configuration_digest,
        default_policy_digest: None,
        required_bindings: Vec::new(),
        allowed_capability_types: Vec::new(),
        failure: WorkflowStepFailureContract {
            error_output: None,
            retry_classification: WorkflowStepRetryClassification::NotRetryable,
            fallback: WorkflowStepFallbackMode::Unsupported,
            failure_branch: false,
        },
        minimum_compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        maximum_compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: label.into(),
            summary: format!("Deterministic {label} descriptor"),
            icon_key: id.into(),
        },
    }
}

fn port(name: &str) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type: WorkflowDataType::Object,
        cardinality: WorkflowStepPortCardinality::Single,
        required: true,
        dynamic: false,
    }
}

fn descriptor_bindings(
    id: &str,
    registry: &WorkflowStepDescriptorRegistry,
    target_descriptor_id: &str,
) -> ApplicationResult<WorkflowStepDescriptorBindings> {
    let pairs = [
        (INPUT_STEP_ID, INPUT_DESCRIPTOR_ID),
        (TARGET_STEP_ID, target_descriptor_id),
        (OUTPUT_STEP_ID, OUTPUT_DESCRIPTOR_ID),
    ];
    WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: id.into(),
        revision: PRESET_CONTRACT_REVISION.into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        bindings: pairs
            .into_iter()
            .map(|(step_id, descriptor_id)| {
                let descriptor = registry
                    .resolve(descriptor_id, PRESET_CONTRACT_REVISION)
                    .ok_or_else(|| {
                        ApplicationError::Internal(format!(
                            "preset descriptor {descriptor_id:?} disappeared"
                        ))
                    })?;
                Ok(WorkflowStepDescriptorBinding {
                    step_id: step_id.into(),
                    descriptor_id: descriptor_id.into(),
                    descriptor_revision: PRESET_CONTRACT_REVISION.into(),
                    semantic_digest: descriptor.semantic_digest().clone(),
                })
            })
            .collect::<ApplicationResult<Vec<_>>>()?,
    })
    .map_err(ApplicationError::Invalid)
}

fn variable_contract(
    id: &str,
    schema_digest: Sha256Digest,
) -> ApplicationResult<WorkflowVariableContract> {
    WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: id.into(),
        revision: PRESET_CONTRACT_REVISION.into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        declarations: vec![
            WorkflowVariableDeclaration {
                name: "request".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::Object,
                value_schema_digest: schema_digest.clone(),
                source_schema_digest: Some(schema_digest.clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "result".into(),
                scope: WorkflowVariableScope::NodeOutput,
                value_type: WorkflowDataType::Object,
                value_schema_digest: schema_digest.clone(),
                source_schema_digest: Some(schema_digest.clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: Some(TARGET_STEP_ID.into()),
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
        ],
        reads: vec![
            WorkflowVariableRead {
                id: "invoke-request".into(),
                variable: "request".into(),
                consumer_step_id: TARGET_STEP_ID.into(),
                consumer_region_id: None,
                target_port: "request".into(),
                path: Vec::new(),
                expected_type: WorkflowDataType::Object,
                expected_schema_digest: schema_digest.clone(),
                required: true,
                mode: WorkflowVariableReadMode::DirectValue,
            },
            WorkflowVariableRead {
                id: "output-result".into(),
                variable: "result".into(),
                consumer_step_id: OUTPUT_STEP_ID.into(),
                consumer_region_id: None,
                target_port: "result".into(),
                path: Vec::new(),
                expected_type: WorkflowDataType::Object,
                expected_schema_digest: schema_digest,
                required: true,
                mode: WorkflowVariableReadMode::DirectValue,
            },
        ],
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .map_err(ApplicationError::Invalid)
}

trait PresetExperienceExt {
    fn registry_id(self) -> &'static str;
}

impl PresetExperienceExt for ApplicationExperience {
    fn registry_id(self) -> &'static str {
        match self {
            Self::Chatbot => "application.preset.chatbot",
            Self::TextGenerator => "application.preset.text-generator",
            Self::ClassicAgent => "application.preset.classic-agent",
            Self::NewAgent => "application.preset.new-agent",
            Self::Chatflow => "application.user-authored.chatflow",
            Self::Workflow => "application.user-authored.workflow",
        }
    }
}
