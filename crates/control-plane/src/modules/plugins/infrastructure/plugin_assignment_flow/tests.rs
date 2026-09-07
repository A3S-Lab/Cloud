use super::authorize_trust::authorize_trust_command_id;
use super::enqueue_apply::enqueue_apply_command_id;
use super::enqueue_plan::{
    enqueue_enablement_plan_command_id, enqueue_plan_command_id, pre_plan_observe_command_id,
};
use super::observe::observe_command_id;
use super::store_plan::store_plan_projection_id;
use super::types::{
    AppliedAssignment, AuthorizeTrustInput, AuthorizedTrust, AwaitConfirmationInput, ConfirmedPlan,
    EnqueueApplyInput, EnqueuePlanInput, LockedPluginAssignment, ObserveInput, PlannedAssignment,
    ResolveHostInput, ResolvedHost, StorePlanInput, StoredPlan,
};
use super::{
    PluginAssignmentFlowConfig, PluginAssignmentFlowConfigOptions, PluginAssignmentFlowRuntime,
    PluginAssignmentFlowRuntimeDependencies, PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
    PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION, PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
    PLUGIN_ASSIGNMENT_ENQUEUE_PLAN, PLUGIN_ASSIGNMENT_LOCK, PLUGIN_ASSIGNMENT_OBSERVE,
    PLUGIN_ASSIGNMENT_RESOLVE_HOST, PLUGIN_ASSIGNMENT_STORE_PLAN,
};
use crate::modules::artifacts::application::INodeArtifactStore;
use crate::modules::artifacts::infrastructure::NodeArtifactObjectStore;
use crate::modules::plugins::application::{
    PLUGIN_ASSIGNMENT_WORKFLOW_NAME, PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
};
use crate::modules::plugins::domain::entities::{
    NewPluginAssignment, NewPluginRegistry, PluginAssignment, PluginRegistry,
};
use crate::modules::plugins::domain::events::PluginAssignmentChanged;
use crate::modules::plugins::domain::repositories::{
    CreatePluginAssignmentWrite, CreatePluginRegistryWrite, IPluginAssignmentRepository,
    IPluginPlanProjectionRepository, IPluginRegistryRepository,
};
use crate::modules::plugins::domain::services::{
    IPluginPolicyStore, IPluginRegistryCatalog, IPluginTrustRootStore, PluginRegistryCatalogError,
};
use crate::modules::plugins::domain::value_objects::{
    PluginCatalogSelection, PluginRegistryEndpoint, PluginTrustRoot,
};
use crate::modules::plugins::infrastructure::{
    PluginPolicyObjectStore, PluginTrustRootObjectStore,
};
use crate::modules::plugins::test_support::VALID_BOOTSTRAP_ROOT;
use crate::modules::plugins::{
    InMemoryPluginAssignmentRepository, InMemoryPluginPlanProjectionRepository,
    InMemoryPluginRegistryRepository,
};
use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::shared_kernel::domain::{
    EnrollmentTokenId, EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OperationId,
    OrganizationId, PluginAssignmentId, PluginRegistryId, PrincipalId, ProjectId, ResourceName,
    Sha256Digest,
};
use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCommandAck, NodeCommandEnvelope, NodeCommandLeaseRequest,
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodePluginHostTrustAuthorized,
};
use a3s_flow::{
    FlowEvent, FlowEventEnvelope, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowSpec,
};
use a3s_runtime::contract::{IsolationLevel, NetworkMode, RuntimeCapabilities, RuntimeUnitClass};
use a3s_use_core::{
    PlanActor, PlanAuthority, PlanPackageChangeKind, PlanPackageRole, PlanPolicyDecision, PlanScope,
    PlanScopeKind, PlannedOperationImpact, PlannedPackageTransition, PlannedStateEvidence,
    PlannedWorkspaceImpact, PluginCatalogRecord, PluginDesiredState, PluginHostApplyResult,
    PluginHostCapabilities, PluginHostEnablementPlanRequest, PluginHostEnablementPlanResult,
    PluginHostEnablementPlanStatus, PluginHostObservationResult, PluginHostObservationStatus,
    PluginHostPackageState, PluginHostPlanRequest, PluginHostPlanResult, PluginManagedScope,
    PluginObservedState, PluginOperationAction, PluginOperationConfirmation,
    PluginOperationPlanBinding, PluginOperationPlanDraft, PluginOperationPlanEnvelope,
    PluginPackageId, PluginReleaseChannel, PluginSurfaceKind, PluginSurfaceRef,
    VerifiedCatalogProvenance, VerifiedPluginCatalogRecord, PLUGIN_HOST_APPLY_RESULT_SCHEMA,
    PLUGIN_HOST_ENABLEMENT_PLAN_RESULT_SCHEMA, PLUGIN_HOST_OBSERVATION_RESULT_SCHEMA,
    PLUGIN_HOST_PLAN_RESULT_SCHEMA, PLUGIN_MANAGED_SCOPE_SCHEMA_V2,
    PLUGIN_OPERATION_CONFIRMATION_SCHEMA,
};
use a3s_use_extension::{
    PluginCatalogHost, PluginCatalogInspection, PluginCatalogPage, PluginCatalogSearch,
    PluginCatalogSnapshot, PluginCatalogSnapshotSource, VerifiedRegistryMetadata,
    MAX_BOOTSTRAP_ROOT_BYTES,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

const CATALOG_FIXTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/plugins/plugin-catalog-okf-v3.json"
));

fn digest(byte: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
}

fn catalog_candidate() -> VerifiedPluginCatalogRecord {
    let record = PluginCatalogRecord::from_json(CATALOG_FIXTURE).expect("catalog fixture");
    let catalog_record_digest = record.descriptor_digest().expect("catalog digest");
    VerifiedPluginCatalogRecord::new(
        record,
        VerifiedCatalogProvenance {
            registry_name: "official".into(),
            registry_url: "https://plugins.a3s.dev/catalog".into(),
            root_sha256: digest('d').as_str().into(),
            root_version: 7,
            timestamp_version: 42,
            snapshot_version: 41,
            targets_version: 39,
            catalog_record_digest,
        },
    )
    .expect("verified catalog record")
}

fn host_capabilities() -> PluginHostCapabilities {
    PluginHostCapabilities::v6("host:node-01", "0.2.2", "use:0.2.2:linux-x86_64")
        .expect("capabilities")
}

fn capabilities_digest() -> String {
    host_capabilities()
        .descriptor_digest()
        .expect("capabilities digest")
}

struct FixtureCatalog {
    candidate: VerifiedPluginCatalogRecord,
}

#[async_trait]
impl IPluginRegistryCatalog for FixtureCatalog {
    async fn refresh(
        &self,
        _registry: &PluginRegistry,
    ) -> Result<VerifiedRegistryMetadata, PluginRegistryCatalogError> {
        Err(PluginRegistryCatalogError::Use {
            code: "fixture.refresh_not_expected".into(),
        })
    }

    async fn search(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError> {
        Err(PluginRegistryCatalogError::Use {
            code: "fixture.search_not_expected".into(),
        })
    }

    async fn search_cached(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _search: &PluginCatalogSearch,
    ) -> Result<PluginCatalogPage, PluginRegistryCatalogError> {
        Err(PluginRegistryCatalogError::Use {
            code: "fixture.search_cached_not_expected".into(),
        })
    }

    async fn inspect(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        _package_id: &str,
        _version: Option<&str>,
        _channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        Err(PluginRegistryCatalogError::Use {
            code: "fixture.inspect_not_expected".into(),
        })
    }

    async fn inspect_cached(
        &self,
        _registry: &PluginRegistry,
        _host: &PluginCatalogHost,
        package_id: &str,
        version: Option<&str>,
        _channel: Option<PluginReleaseChannel>,
    ) -> Result<PluginCatalogInspection, PluginRegistryCatalogError> {
        if package_id != self.candidate.record.package_id
            || version.is_some_and(|value| value != self.candidate.record.version)
        {
            return Err(PluginRegistryCatalogError::PackageNotFound);
        }
        Ok(PluginCatalogInspection {
            snapshot: PluginCatalogSnapshot {
                metadata: VerifiedRegistryMetadata {
                    registry_name: self.candidate.provenance.registry_name.clone(),
                    registry_url: self.candidate.provenance.registry_url.clone(),
                    root_sha256: self.candidate.provenance.root_sha256.clone(),
                    root_version: self.candidate.provenance.root_version,
                    timestamp_version: self.candidate.provenance.timestamp_version,
                    snapshot_version: self.candidate.provenance.snapshot_version,
                    targets_version: self.candidate.provenance.targets_version,
                    package_targets: 1,
                },
                source: PluginCatalogSnapshotSource::Cached,
                host_target: "linux-x86_64".into(),
                use_version: "0.2.2".into(),
                catalog_records: 1,
                verified_at_unix_seconds: 1,
                age_seconds: 0,
                snapshot_digest: digest('1').as_str().into(),
            },
            plugin: self.candidate.clone(),
        })
    }
}

fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded(
        PLUGIN_ASSIGNMENT_WORKFLOW_NAME,
        PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
        "plugins",
        "plugin_assignment",
    )
}

fn runtime_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("test-plugin-host-runtime").expect("provider"),
        provider_build: "test-plugin-host-1".into(),
        unit_classes: vec![RuntimeUnitClass::Task],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Sandbox],
        network_modes: vec![NetworkMode::None],
        mount_kinds: Vec::new(),
        health_check_kinds: Vec::new(),
        resource_controls: vec![
            a3s_runtime::contract::ResourceControl::Cpu,
            a3s_runtime::contract::ResourceControl::Memory,
            a3s_runtime::contract::ResourceControl::Pids,
            a3s_runtime::contract::ResourceControl::ExecutionTimeout,
        ],
        features: vec![
            a3s_runtime::contract::RuntimeFeature::DurableIdentity,
            a3s_runtime::contract::RuntimeFeature::Remove,
        ],
    }
}

fn event(organization_id: OrganizationId) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: "test.plugin.fixture".into(),
        schema_version: 1,
        scope: a3s_cloud_contracts::CloudScopeRef::Organization {
            organization_id: organization_id.as_uuid(),
        },
        aggregate_id: Uuid::now_v7(),
        aggregate_version: 1,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}

struct Fixture {
    runtime: PluginAssignmentFlowRuntime,
    assignment: PluginAssignment,
    operation_id: OperationId,
    nodes: Arc<InMemoryNodeRepository>,
    projections: Arc<InMemoryPluginPlanProjectionRepository>,
    node_id: NodeId,
    agent_instance_id: Uuid,
}

impl Fixture {
    async fn create() -> Self {
        Self::create_with_desired(PluginDesiredState::Enabled).await
    }

    async fn create_with_desired(desired_state: PluginDesiredState) -> Self {
        let organization_id = OrganizationId::new();
        let operation_id = OperationId::new();
        let node_id = NodeId::new();
        let registry_id = PluginRegistryId::new();
        let now = Utc::now();
        let nodes = Arc::new(InMemoryNodeRepository::new());
        let agent_instance_id = enroll_ready_node(&nodes, organization_id, node_id, now).await;
        let trust_roots = Arc::new(
            PluginTrustRootObjectStore::in_memory(MAX_BOOTSTRAP_ROOT_BYTES).expect("trust roots"),
        );
        let trust_root = PluginTrustRoot::from_digest(
            Sha256Digest::from_bytes(VALID_BOOTSTRAP_ROOT),
            1,
        )
        .expect("trust root");
        trust_roots
            .put(&trust_root, VALID_BOOTSTRAP_ROOT.to_vec())
            .await
            .expect("store trust root");
        let policy_bytes = b"default = \"deny\"\nallow.plugin.selftest = true\n".to_vec();
        let policy_digest = Sha256Digest::from_bytes(&policy_bytes);
        let policies = Arc::new(PluginPolicyObjectStore::in_memory(1024).expect("policies"));
        policies
            .put(&policy_digest, policy_bytes)
            .await
            .expect("store policy");
        let objects: Arc<dyn object_store::ObjectStore> =
            Arc::new(object_store::memory::InMemory::new());
        let artifacts: Arc<dyn INodeArtifactStore> = Arc::new(
            NodeArtifactObjectStore::from_client(
                crate::infrastructure::ImmutableObjectClient::from_store(objects, "artifacts")
                    .expect("artifact client"),
                16 * 1024 * 1024,
            )
            .expect("artifacts"),
        );
        let registries = Arc::new(InMemoryPluginRegistryRepository::new());
        let actor_id = PrincipalId::new();
        let registry = PluginRegistry::enroll(NewPluginRegistry {
            organization_id,
            id: registry_id,
            name: ResourceName::parse("fixture-registry").expect("name"),
            endpoint: PluginRegistryEndpoint::parse("https://registry.example.test/tuf/")
                .expect("endpoint"),
            trust_root,
            actor_id,
            request_id: Uuid::now_v7(),
            enrolled_at: now,
        })
        .expect("registry");
        registries
            .create(CreatePluginRegistryWrite {
                registry: registry.clone(),
                event: crate::modules::plugins::domain::events::PluginRegistryEnrolled::envelope(
                    &registry,
                )
                .expect("registry event"),
                authorization:
                    crate::modules::plugins::domain::services::PluginRegistryEnrollmentAuthorization::new(
                        organization_id,
                        actor_id,
                    )
                    .expect("authorization"),
                idempotency: CreatePluginRegistryWrite::idempotency_for(&registry, "flow-registry")
                    .expect("registry idem"),
            })
            .await
            .expect("create registry");
        let candidate = catalog_candidate();
        let selection = PluginCatalogSelection {
            package_id: PluginPackageId::parse(candidate.record.package_id.as_str())
                .expect("package"),
            catalog_record_digest: Sha256Digest::parse(
                candidate.record.descriptor_digest().expect("catalog digest"),
            )
            .expect("catalog digest"),
            version: candidate.record.version.clone(),
            package_digest: Sha256Digest::parse(
                candidate
                    .record
                    .package
                    .sha256
                    .clone()
                    .expect("package digest"),
            )
            .expect("package digest"),
            manifest_digest: Sha256Digest::parse(
                candidate
                    .record
                    .package
                    .manifest_sha256
                    .clone()
                    .expect("manifest digest"),
            )
            .expect("manifest digest"),
            selected_surfaces: vec![PluginSurfaceRef {
                kind: PluginSurfaceKind::Skill,
                id: "research".into(),
            }],
        };
        let assignment = PluginAssignment::create(NewPluginAssignment {
            organization_id,
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            id: PluginAssignmentId::new(),
            registry_id,
            target_host_id: node_id,
            workspace_scope: PluginManagedScope {
                schema: PLUGIN_MANAGED_SCOPE_SCHEMA_V2.into(),
                host_id: "host:node-01".into(),
                scope_kind: PlanScopeKind::Workspace,
                scope_id: "workspace:research".into(),
                authority_id: "cloud:organization-01".into(),
                fence_generation: 7,
                fence_digest: digest('d').as_str().into(),
            },
            selection,
            policy_digest,
            desired_state,
            actor_id: PrincipalId::new(),
            request_id: Uuid::now_v7(),
            operation_id,
            created_at: now,
        })
        .expect("assignment");
        let assignments = Arc::new(InMemoryPluginAssignmentRepository::new());
        let event = PluginAssignmentChanged::envelope(&assignment).expect("event");
        let idempotency =
            CreatePluginAssignmentWrite::idempotency_for(&assignment, "flow-seed").expect("idem");
        assignments
            .create(CreatePluginAssignmentWrite {
                assignment: assignment.clone(),
                event,
                idempotency,
            })
            .await
            .expect("create");
        let catalog: Arc<dyn IPluginRegistryCatalog> = Arc::new(FixtureCatalog { candidate });
        let projections = Arc::new(InMemoryPluginPlanProjectionRepository::new());
        let runtime = PluginAssignmentFlowRuntime::new(
            PluginAssignmentFlowRuntimeDependencies {
                assignments: assignments as Arc<dyn IPluginAssignmentRepository>,
                registries: registries as Arc<dyn IPluginRegistryRepository>,
                nodes: nodes.clone() as Arc<dyn INodeRepository>,
                node_control: nodes.clone() as Arc<dyn INodeControlRepository>,
                trust_roots: trust_roots as Arc<dyn IPluginTrustRootStore>,
                policies: policies as Arc<dyn IPluginPolicyStore>,
                artifacts,
                catalog,
                projections: projections.clone() as Arc<dyn IPluginPlanProjectionRepository>,
            },
            PluginAssignmentFlowConfig::new(PluginAssignmentFlowConfigOptions {
                observation_poll_ms: 1,
                command_ttl_ms: 60_000,
                convergence_timeout_ms: 60_000,
            })
            .expect("config"),
        );
        Self {
            runtime,
            assignment,
            operation_id,
            nodes,
            projections,
            node_id,
            agent_instance_id,
        }
    }

    fn locked(&self) -> LockedPluginAssignment {
        let desired_state = match self.assignment.desired_state {
            PluginDesiredState::Enabled => "enabled",
            PluginDesiredState::InstalledDisabled => "installed-disabled",
            PluginDesiredState::Absent => "absent",
        };
        LockedPluginAssignment {
            organization_id: self.assignment.organization_id,
            assignment_id: self.assignment.id,
            operation_id: self.operation_id,
            assignment_generation: self.assignment.assignment_generation,
            registry_id: self.assignment.registry_id,
            target_host_id: self.node_id,
            package_id: self.assignment.package_id().as_str().to_owned(),
            desired_state: desired_state.into(),
            locked_at: self.assignment.created_at,
        }
    }

    fn flow_input(&self) -> serde_json::Value {
        json!({
            "organizationId": self.assignment.organization_id,
            "assignmentId": self.assignment.id,
            "operationId": self.operation_id,
            "assignmentGeneration": self.assignment.assignment_generation,
        })
    }
}

async fn enroll_ready_node(
    nodes: &InMemoryNodeRepository,
    organization_id: OrganizationId,
    proposed_node_id: NodeId,
    enrolled_at: chrono::DateTime<Utc>,
) -> Uuid {
    let capabilities = runtime_capabilities();
    capabilities.validate().expect("capabilities");
    let token_id = EnrollmentTokenId::new();
    let secret = format!("a3sn_{}", token_id.as_uuid().simple().to_string().repeat(2));
    let credential = EnrollmentTokenCredential::from_secret(&secret).expect("credential");
    nodes
        .issue_enrollment_token(
            EnrollmentToken::new(
                token_id,
                organization_id,
                "plugin-host",
                credential.clone(),
                enrolled_at,
                enrolled_at + Duration::minutes(5),
            )
            .expect("token"),
            event(organization_id),
            IdempotencyRequest::new(
                "test.plugin.enrollment",
                token_id.to_string(),
                token_id.to_string().as_bytes(),
            )
            .expect("idempotency"),
        )
        .await
        .expect("issue token");
    let stored = NodeCapabilities::new(
        capabilities.provider_id.to_string(),
        capabilities.provider_build.clone(),
        serde_json::to_value(&capabilities).expect("json"),
    )
    .expect("node capabilities");
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id,
                name: NodeName::new("plugin-host").expect("name"),
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: stored.clone(),
                request_digest: format!("sha256:{}", "c".repeat(64)),
                requested_at: enrolled_at,
            },
        )
        .await
        .expect("reserve");
    nodes
        .record_heartbeat(crate::modules::fleet::domain::repositories::NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: stored,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await
        .expect("heartbeat");
    agent_instance_id
}

async fn lease_command(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(Uuid, NodeCommandEnvelope), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let lease = fixture
        .nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: fixture.node_id.as_uuid(),
                agent_instance_id: fixture.agent_instance_id,
                after_sequence: 0,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(5),
        )
        .await?;
    let command = lease
        .commands
        .into_iter()
        .find(|command| command.command_id == command_id.as_uuid())
        .ok_or("command was not leased")?;
    Ok((lease.lease_id, command))
}

async fn acknowledge_capabilities(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let (lease_id, command) = lease_command(fixture, command_id).await?;
    let capabilities = host_capabilities();
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id,
                node_id: fixture.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at: now,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::PluginHostCapabilitiesInspected {
                        capabilities,
                    }),
                },
            },
            now,
        )
        .await?;
    Ok(())
}

async fn acknowledge_authorize_trust(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    let (lease_id, command) = lease_command(fixture, command_id).await?;
    let trust_root_digest = Sha256Digest::from_bytes(VALID_BOOTSTRAP_ROOT)
        .as_str()
        .to_owned();
    let policy_bytes = b"default = \"deny\"\nallow.plugin.selftest = true\n";
    let policy_digest = Sha256Digest::from_bytes(policy_bytes).as_str().to_owned();
    let authorized = NodePluginHostTrustAuthorized::new(
        fixture.assignment.assignment_generation,
        trust_root_digest,
        policy_digest,
        now,
    )?;
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id,
                node_id: fixture.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at: now,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::PluginHostTrustAuthorized { authorized }),
                },
            },
            now,
        )
        .await?;
    Ok(())
}

async fn acknowledge_plan(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(), Box<dyn std::error::Error>> {
    acknowledge_plan_with_decision(fixture, command_id, PlanPolicyDecision::Allow).await
}

async fn acknowledge_plan_with_decision(
    fixture: &Fixture,
    command_id: NodeCommandId,
    decision: PlanPolicyDecision,
) -> Result<(), Box<dyn std::error::Error>> {
    let (lease_id, command) = lease_command(fixture, command_id).await?;
    let NodeCommandPayload::PluginHostPlan { request } = command.payload.clone() else {
        return Err("leased command was not a Plugin Host plan".into());
    };
    let issued_at_ms = u64::try_from(command.issued_at.timestamp_millis())?;
    let plan = plan_result_for_request(&request, issued_at_ms + 1, decision)?;
    let completed_at = plugin_host_timestamp_ms(plan.plan.plan.created_at_ms)?
        + Duration::milliseconds(1);
    let capabilities = host_capabilities();
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id,
                node_id: fixture.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::PluginHostPlanned {
                        capabilities,
                        plan: Box::new(plan),
                    }),
                },
            },
            completed_at,
        )
        .await?;
    Ok(())
}

async fn acknowledge_apply(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(), Box<dyn std::error::Error>> {
    let (lease_id, command) = lease_command(fixture, command_id).await?;
    let NodeCommandPayload::PluginHostApply { request } = command.payload.clone() else {
        return Err("leased command was not a Plugin Host apply".into());
    };
    let completed_at = command.issued_at + Duration::milliseconds(50);
    let completed_at_ms = u64::try_from(completed_at.timestamp_millis())?;
    let state = match fixture.assignment.desired_state {
        PluginDesiredState::Absent => PluginHostPackageState {
            version: None,
            package_generation: None,
            package_digest: None,
            manifest_digest: None,
            receipt_digest: None,
            capability_generation: 15,
            capability_revision: digest('c').as_str().into(),
            desired: PluginDesiredState::Absent,
            observed: PluginObservedState::Removed,
            selected_surfaces: Vec::new(),
        },
        PluginDesiredState::InstalledDisabled => PluginHostPackageState {
            version: Some(fixture.assignment.selection.version.clone()),
            package_generation: Some(13),
            package_digest: Some(fixture.assignment.selection.package_digest.as_str().into()),
            manifest_digest: Some(fixture.assignment.selection.manifest_digest.as_str().into()),
            receipt_digest: Some(digest('b').as_str().into()),
            capability_generation: 14,
            capability_revision: digest('c').as_str().into(),
            desired: PluginDesiredState::InstalledDisabled,
            observed: PluginObservedState::Installed,
            selected_surfaces: fixture.assignment.selection.selected_surfaces.clone(),
        },
        PluginDesiredState::Enabled => PluginHostPackageState {
            version: Some(fixture.assignment.selection.version.clone()),
            package_generation: Some(13),
            package_digest: Some(fixture.assignment.selection.package_digest.as_str().into()),
            manifest_digest: Some(fixture.assignment.selection.manifest_digest.as_str().into()),
            receipt_digest: Some(digest('b').as_str().into()),
            capability_generation: 14,
            capability_revision: digest('c').as_str().into(),
            desired: PluginDesiredState::Enabled,
            observed: PluginObservedState::Ready,
            selected_surfaces: fixture.assignment.selection.selected_surfaces.clone(),
        },
    };
    let applied = PluginHostApplyResult {
        schema: PLUGIN_HOST_APPLY_RESULT_SCHEMA.into(),
        request_id: request.request_id.clone(),
        assignment_generation: request.assignment_generation,
        capabilities_digest: request.capabilities_digest.clone(),
        scope: request.scope.clone(),
        package_id: request.package_id.clone(),
        operation_id: request.operation_id.clone(),
        plan_digest: request.plan_digest.clone(),
        completed_at_ms,
        operation_result_digest: digest('a').as_str().into(),
        state,
        replayed: false,
    };
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id,
                node_id: fixture.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at: completed_at + Duration::milliseconds(1),
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::PluginHostApplied {
                        capabilities: host_capabilities(),
                        applied: Box::new(applied),
                    }),
                },
            },
            completed_at + Duration::milliseconds(1),
        )
        .await?;
    Ok(())
}

async fn acknowledge_observe(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(), Box<dyn std::error::Error>> {
    acknowledge_observe_with_state(
        fixture,
        command_id,
        PluginHostPackageState {
            version: Some(fixture.assignment.selection.version.clone()),
            package_generation: Some(13),
            package_digest: Some(fixture.assignment.selection.package_digest.as_str().into()),
            manifest_digest: Some(fixture.assignment.selection.manifest_digest.as_str().into()),
            receipt_digest: Some(digest('b').as_str().into()),
            capability_generation: 14,
            capability_revision: digest('c').as_str().into(),
            desired: PluginDesiredState::Enabled,
            observed: PluginObservedState::Ready,
            selected_surfaces: fixture.assignment.selection.selected_surfaces.clone(),
        },
    )
    .await
}

async fn acknowledge_pre_plan_absent(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(), Box<dyn std::error::Error>> {
    acknowledge_observe_with_state(
        fixture,
        command_id,
        PluginHostPackageState {
            version: None,
            package_generation: None,
            package_digest: None,
            manifest_digest: None,
            receipt_digest: None,
            capability_generation: 1,
            capability_revision: digest('c').as_str().into(),
            desired: PluginDesiredState::Absent,
            observed: PluginObservedState::Removed,
            selected_surfaces: Vec::new(),
        },
    )
    .await
}

async fn acknowledge_observe_with_state(
    fixture: &Fixture,
    command_id: NodeCommandId,
    state: PluginHostPackageState,
) -> Result<(), Box<dyn std::error::Error>> {
    let (lease_id, command) = lease_command(fixture, command_id).await?;
    let NodeCommandPayload::PluginHostObserve { request } = command.payload.clone() else {
        return Err("leased command was not a Plugin Host observation".into());
    };
    let completed_at = command.issued_at + Duration::milliseconds(50);
    let observed_at_ms = u64::try_from(completed_at.timestamp_millis())?;
    let observation = PluginHostObservationResult {
        schema: PLUGIN_HOST_OBSERVATION_RESULT_SCHEMA.into(),
        request_id: request.request_id.clone(),
        assignment_generation: request.assignment_generation,
        capabilities_digest: request.capabilities_digest.clone(),
        scope: request.scope.clone(),
        package_id: request.package_id.clone(),
        observed_at_ms,
        status: PluginHostObservationStatus::Available { state },
    };
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id,
                node_id: fixture.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at: completed_at + Duration::milliseconds(1),
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::PluginHostObserved {
                        capabilities: host_capabilities(),
                        observation: Box::new(observation),
                    }),
                },
            },
            completed_at + Duration::milliseconds(1),
        )
        .await?;
    Ok(())
}

async fn ack_pre_plan_then_package_plan(
    fixture: &Fixture,
) -> Result<NodeCommandId, Box<dyn std::error::Error>> {
    let observe_id = pre_plan_observe_command_id(fixture.operation_id.as_uuid());
    acknowledge_pre_plan_absent(fixture, observe_id).await?;
    Ok(enqueue_plan_command_id(fixture.operation_id.as_uuid()))
}

fn installed_enabled_state(fixture: &Fixture) -> PluginHostPackageState {
    PluginHostPackageState {
        version: Some(fixture.assignment.selection.version.clone()),
        package_generation: Some(13),
        package_digest: Some(fixture.assignment.selection.package_digest.as_str().into()),
        manifest_digest: Some(fixture.assignment.selection.manifest_digest.as_str().into()),
        receipt_digest: Some(digest('b').as_str().into()),
        capability_generation: 14,
        capability_revision: digest('c').as_str().into(),
        desired: PluginDesiredState::Enabled,
        observed: PluginObservedState::Ready,
        selected_surfaces: fixture.assignment.selection.selected_surfaces.clone(),
    }
}

async fn ack_pre_plan_then_enablement_plan(
    fixture: &Fixture,
) -> Result<NodeCommandId, Box<dyn std::error::Error>> {
    let observe_id = pre_plan_observe_command_id(fixture.operation_id.as_uuid());
    acknowledge_observe_with_state(fixture, observe_id, installed_enabled_state(fixture)).await?;
    Ok(enqueue_enablement_plan_command_id(
        fixture.operation_id.as_uuid(),
    ))
}

fn enablement_plan_result_for_request(
    request: &PluginHostEnablementPlanRequest,
    planned_at_ms: u64,
    decision: PlanPolicyDecision,
) -> Result<PluginHostEnablementPlanResult, Box<dyn std::error::Error>> {
    let candidate = catalog_candidate();
    let surfaces = vec![PluginSurfaceRef {
        kind: PluginSurfaceKind::Skill,
        id: "research".into(),
    }];
    let retained = candidate.selected_state(&surfaces)?;
    let transition = PlannedPackageTransition::resolved(
        request.package_id.as_str(),
        PlanPackageRole::Root,
        PlanPackageChangeKind::Retain,
        Some(retained.clone()),
        Some(retained),
        None,
    )?;
    let action = if request.enabled {
        PluginOperationAction::Enable
    } else {
        PluginOperationAction::Disable
    };
    let (enabled_before, enabled_after) = if request.enabled {
        (false, true)
    } else {
        (true, false)
    };
    let draft = PluginOperationPlanDraft::new_unbound(
        action,
        request.package_id.as_str(),
        format!("use/{}", request.package_id.as_str()),
        vec![transition],
        vec![PlannedWorkspaceImpact {
            scope_id: request.scope.scope_id.clone(),
            grant_before_digest: Some(digest('a').as_str().into()),
            grant_after_digest: Some(digest('b').as_str().into()),
            enabled_before,
            enabled_after,
        }],
        PlannedOperationImpact {
            download_bytes: 0,
            installed_bytes_after: candidate.record.package.expanded_bytes,
            reclaimed_bytes: 0,
            drain_required: false,
            retained_data: !request.enabled,
            okf_changes: Vec::new(),
        },
        PlannedStateEvidence {
            state_revision: 3,
            capability_generation: 14,
            receipt_digest: Some(digest('b').as_str().into()),
        },
    )?;
    let plan = draft.bind(PluginOperationPlanBinding {
        operation_id: format!("use-operation:enablement:{}", request.request_id),
        created_at_ms: planned_at_ms,
        expires_at_ms: planned_at_ms + 10 * 60 * 1_000,
        scope: PlanScope {
            kind: PlanScopeKind::Workspace,
            id: request.scope.scope_id.clone(),
        },
        authority: PlanAuthority {
            actor: PlanActor::User,
            decision,
            policy_digest: digest('c').as_str().into(),
            confirmation_required: decision == PlanPolicyDecision::Ask,
        },
    })?;
    let state = PluginHostPackageState {
        version: Some(candidate.record.version.clone()),
        package_generation: Some(request.expected_package_generation),
        package_digest: candidate.record.package.sha256.clone(),
        manifest_digest: candidate.record.package.manifest_sha256.clone(),
        receipt_digest: Some(digest('b').as_str().into()),
        capability_generation: 14,
        capability_revision: digest('c').as_str().into(),
        desired: if request.enabled {
            PluginDesiredState::InstalledDisabled
        } else {
            PluginDesiredState::Enabled
        },
        observed: PluginObservedState::Ready,
        selected_surfaces: surfaces,
    };
    Ok(PluginHostEnablementPlanResult {
        schema: PLUGIN_HOST_ENABLEMENT_PLAN_RESULT_SCHEMA.into(),
        request_id: request.request_id.clone(),
        assignment_generation: request.assignment_generation,
        capabilities_digest: request.capabilities_digest.clone(),
        scope: request.scope.clone(),
        package_id: request.package_id.clone(),
        expected_package_generation: request.expected_package_generation,
        enabled: request.enabled,
        planned_at_ms,
        status: PluginHostEnablementPlanStatus::Planned,
        state,
        plan: Some(PluginOperationPlanEnvelope::new(plan)?),
        replayed: false,
    })
}

async fn acknowledge_enablement_plan(
    fixture: &Fixture,
    command_id: NodeCommandId,
) -> Result<(), Box<dyn std::error::Error>> {
    let (lease_id, command) = lease_command(fixture, command_id).await?;
    let NodeCommandPayload::PluginHostPlanEnablement { request } = command.payload.clone() else {
        return Err("leased command was not a Plugin Host enablement-plan".into());
    };
    let issued_at_ms = u64::try_from(command.issued_at.timestamp_millis())?;
    let enablement_plan =
        enablement_plan_result_for_request(&request, issued_at_ms + 1, PlanPolicyDecision::Allow)?;
    let completed_at = plugin_host_timestamp_ms(enablement_plan.planned_at_ms)?
        + Duration::milliseconds(1);
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id,
                node_id: fixture.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::PluginHostEnablementPlanned {
                        capabilities: host_capabilities(),
                        enablement_plan: Box::new(enablement_plan),
                    }),
                },
            },
            completed_at,
        )
        .await?;
    Ok(())
}

fn plugin_host_timestamp_ms(
    milliseconds: u64,
) -> Result<chrono::DateTime<Utc>, Box<dyn std::error::Error>> {
    let milliseconds = i64::try_from(milliseconds)?;
    chrono::DateTime::from_timestamp_millis(milliseconds)
        .ok_or_else(|| "Plugin Host plan creation time exceeds supported bounds".into())
}

fn plan_result_for_request(
    request: &PluginHostPlanRequest,
    created_at_ms: u64,
    decision: PlanPolicyDecision,
) -> Result<PluginHostPlanResult, Box<dyn std::error::Error>> {
    let (transition, impact, receipt_digest, enabled_before, enabled_after, grant_before, grant_after) =
        match request.action {
            PluginOperationAction::Install => {
                let candidate = request
                    .candidate
                    .as_ref()
                    .ok_or("plan request is missing an install candidate")?;
                (
                    candidate
                        .install_transition(PlanPackageRole::Root, &request.selected_surfaces)?,
                    PlannedOperationImpact {
                        download_bytes: candidate.record.archive.length,
                        installed_bytes_after: candidate.record.package.expanded_bytes,
                        reclaimed_bytes: 0,
                        drain_required: false,
                        retained_data: false,
                        okf_changes: Vec::new(),
                    },
                    None,
                    false,
                    true,
                    None,
                    Some(digest('b').as_str().into()),
                )
            }
            PluginOperationAction::Upgrade => {
                let candidate = request
                    .candidate
                    .as_ref()
                    .ok_or("plan request is missing an upgrade candidate")?;
                let mut before =
                    candidate.selected_state(&request.selected_surfaces)?;
                before.release.version = "0.9.0".into();
                before.release.package_sha256 = digest('9').as_str().into();
                before.release.manifest_sha256 = digest('8').as_str().into();
                let after = candidate.selected_state(&request.selected_surfaces)?;
                (
                    PlannedPackageTransition::resolved(
                        request.package_id.as_str(),
                        PlanPackageRole::Root,
                        PlanPackageChangeKind::Replace,
                        Some(before),
                        Some(after),
                        Some(a3s_use_core::PluginPlanSource::Registry {
                            provenance: candidate.provenance.clone(),
                            archive: candidate.record.archive.clone(),
                        }),
                    )?,
                    PlannedOperationImpact {
                        download_bytes: candidate.record.archive.length,
                        installed_bytes_after: candidate.record.package.expanded_bytes,
                        reclaimed_bytes: 0,
                        drain_required: false,
                        retained_data: false,
                        okf_changes: Vec::new(),
                    },
                    Some(digest('b').as_str().into()),
                    true,
                    true,
                    Some(digest('a').as_str().into()),
                    Some(digest('b').as_str().into()),
                )
            }
            PluginOperationAction::Uninstall => {
                let evidence = catalog_candidate();
                let surfaces = vec![PluginSurfaceRef {
                    kind: PluginSurfaceKind::Skill,
                    id: "research".into(),
                }];
                (
                    evidence.remove_transition(PlanPackageRole::Root, &surfaces)?,
                    PlannedOperationImpact {
                        download_bytes: 0,
                        installed_bytes_after: 0,
                        reclaimed_bytes: evidence.record.package.expanded_bytes,
                        drain_required: false,
                        retained_data: true,
                        okf_changes: Vec::new(),
                    },
                    Some(digest('b').as_str().into()),
                    true,
                    false,
                    Some(digest('a').as_str().into()),
                    None,
                )
            }
            PluginOperationAction::Enable | PluginOperationAction::Disable => {
                return Err("enable/disable must use enablement-plan ack helper".into());
            }
        };
    let draft = PluginOperationPlanDraft::new(
        request.action,
        request.package_id.as_str(),
        request.package_id.component_id(),
        vec![transition],
        Vec::new(),
        vec![PlannedWorkspaceImpact {
            scope_id: request.scope.scope_id.clone(),
            grant_before_digest: grant_before,
            grant_after_digest: grant_after,
            enabled_before,
            enabled_after,
        }],
        impact,
        PlannedStateEvidence {
            state_revision: 3,
            capability_generation: 12,
            receipt_digest,
        },
    )?;
    let plan = draft.bind(PluginOperationPlanBinding {
        operation_id: format!("use-operation:plan:{}", request.request_id),
        created_at_ms,
        expires_at_ms: created_at_ms + 10 * 60 * 1_000,
        scope: PlanScope {
            kind: PlanScopeKind::Workspace,
            id: request.scope.scope_id.clone(),
        },
        authority: PlanAuthority {
            actor: PlanActor::User,
            decision,
            policy_digest: digest('c').as_str().into(),
            confirmation_required: decision == PlanPolicyDecision::Ask,
        },
    })?;
    Ok(PluginHostPlanResult {
        schema: PLUGIN_HOST_PLAN_RESULT_SCHEMA.into(),
        request_id: request.request_id.clone(),
        assignment_generation: request.assignment_generation,
        capabilities_digest: request.capabilities_digest.clone(),
        scope: request.scope.clone(),
        package_id: request.package_id.clone(),
        plan: PluginOperationPlanEnvelope::new(plan)?,
        replayed: false,
    })
}

fn drifted_installed_state(fixture: &Fixture) -> PluginHostPackageState {
    PluginHostPackageState {
        version: Some("0.9.0".into()),
        package_generation: Some(11),
        package_digest: Some(digest('9').as_str().into()),
        manifest_digest: Some(digest('8').as_str().into()),
        receipt_digest: Some(digest('b').as_str().into()),
        capability_generation: 12,
        capability_revision: digest('c').as_str().into(),
        desired: PluginDesiredState::Enabled,
        observed: PluginObservedState::Ready,
        selected_surfaces: fixture.assignment.selection.selected_surfaces.clone(),
    }
}

async fn ack_pre_plan_then_package_plan_with_state(
    fixture: &Fixture,
    state: PluginHostPackageState,
) -> Result<NodeCommandId, Box<dyn std::error::Error>> {
    let observe_id = pre_plan_observe_command_id(fixture.operation_id.as_uuid());
    acknowledge_observe_with_state(fixture, observe_id, state).await?;
    Ok(enqueue_plan_command_id(fixture.operation_id.as_uuid()))
}

fn resolved_host(fixture: &Fixture) -> ResolvedHost {
    ResolvedHost {
        locked: Box::new(fixture.locked()),
        command_id: NodeCommandId::from_uuid(fixture.operation_id.as_uuid()),
        host_id: "host:node-01".into(),
        manager_version: "0.2.2".into(),
        manager_build_id: "use:0.2.2:linux-x86_64".into(),
        capabilities_digest: capabilities_digest(),
        resolved_at: Utc::now(),
    }
}

fn authorized_trust(fixture: &Fixture) -> AuthorizedTrust {
    AuthorizedTrust {
        resolved: Box::new(resolved_host(fixture)),
        command_id: authorize_trust_command_id(fixture.operation_id.as_uuid()),
        trust_root_digest: Sha256Digest::from_bytes(VALID_BOOTSTRAP_ROOT)
            .as_str()
            .to_owned(),
        policy_digest: Sha256Digest::from_bytes(
            b"default = \"deny\"\nallow.plugin.selftest = true\n",
        )
        .as_str()
        .to_owned(),
        authorized_at: Utc::now(),
    }
}

#[tokio::test]
async fn lock_step_reloads_current_assignment_generation() {
    let fixture = Fixture::create().await;
    let output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "lock",
            PLUGIN_ASSIGNMENT_LOCK,
            fixture.flow_input(),
            vec![],
        ))
        .await
        .expect("lock");
    assert_eq!(output["state"], "ready");
    assert_eq!(
        output["locked"]["packageId"],
        fixture.assignment.package_id().as_str()
    );
}

#[tokio::test]
async fn resolve_host_enqueues_capabilities_inspect_and_accepts_exact_ack() {
    let fixture = Fixture::create().await;
    let input = ResolveHostInput {
        locked: Box::new(fixture.locked()),
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "resolve-host-1",
            PLUGIN_ASSIGNMENT_RESOLVE_HOST,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pending");
    assert_eq!(pending["state"], "pending");

    let command_id = NodeCommandId::from_uuid(fixture.operation_id.as_uuid());
    acknowledge_capabilities(&fixture, command_id)
        .await
        .expect("ack");

    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "resolve-host-1",
            PLUGIN_ASSIGNMENT_RESOLVE_HOST,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["resolved"]["hostId"], "host:node-01");
    assert_eq!(ready["resolved"]["managerVersion"], "0.2.2");
}

#[tokio::test]
async fn authorize_trust_admits_artifacts_enqueues_and_accepts_exact_ack() {
    let fixture = Fixture::create().await;
    let input = AuthorizeTrustInput {
        resolved: Box::new(resolved_host(&fixture)),
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "authorize-trust-1",
            PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pending");
    assert_eq!(pending["state"], "pending");

    let command_id = authorize_trust_command_id(fixture.operation_id.as_uuid());
    acknowledge_authorize_trust(&fixture, command_id)
        .await
        .expect("ack");

    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "authorize-trust-1",
            PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["authorized"]["trustRootDigest"],
        Sha256Digest::from_bytes(VALID_BOOTSTRAP_ROOT).as_str()
    );
    assert_eq!(
        ready["authorized"]["policyDigest"],
        Sha256Digest::from_bytes(b"default = \"deny\"\nallow.plugin.selftest = true\n").as_str()
    );
    assert_eq!(
        ready["authorized"]["commandId"],
        command_id.as_uuid().to_string()
    );
}

#[tokio::test]
async fn enqueue_plan_loads_catalog_candidate_and_accepts_exact_ack() {
    let fixture = Fixture::create().await;
    let input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(&fixture)),
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pending observe");
    assert_eq!(pending["state"], "pending");
    assert!(pending["reason"]
        .as_str()
        .unwrap()
        .contains("pre-plan observation"));

    let command_id = ack_pre_plan_then_package_plan(&fixture)
        .await
        .expect("pre-plan ack");
    let pending_plan = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pending plan");
    assert_eq!(pending_plan["state"], "pending");

    acknowledge_plan(&fixture, command_id)
        .await
        .expect("ack");

    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["planned"]["action"], "install");
    assert_eq!(ready["planned"]["planKind"], "package");
    assert_eq!(
        ready["planned"]["commandId"],
        command_id.as_uuid().to_string()
    );
    assert!(!ready["planned"]["planDigest"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn enqueue_enablement_plan_selects_disable_and_accepts_planned_ack() {
    let fixture = Fixture::create_with_desired(PluginDesiredState::InstalledDisabled).await;
    let input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(&fixture)),
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pending observe");
    assert_eq!(pending["state"], "pending");

    let command_id = ack_pre_plan_then_enablement_plan(&fixture)
        .await
        .expect("pre-plan present ack");
    let pending_plan = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pending enablement plan");
    assert_eq!(pending_plan["state"], "pending");
    assert!(pending_plan["reason"]
        .as_str()
        .unwrap()
        .contains("enablement-plan"));

    acknowledge_enablement_plan(&fixture, command_id)
        .await
        .expect("enablement ack");

    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("enablement ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["planned"]["action"], "disable");
    assert_eq!(ready["planned"]["planKind"], "enablement");
    assert_eq!(
        ready["planned"]["commandId"],
        command_id.as_uuid().to_string()
    );
    assert!(!ready["planned"]["planDigest"].as_str().unwrap().is_empty());

    let planned: PlannedAssignment =
        serde_json::from_value(ready["planned"].clone()).expect("planned");
    let stored = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "store-plan",
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            serde_json::to_value(&StorePlanInput {
                planned: Box::new(planned.clone()),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("store enablement plan");
    assert_eq!(stored["state"], "ready");
    assert_eq!(stored["stored"]["planDigest"], planned.plan_digest);
    let projection_id = store_plan_projection_id(fixture.operation_id.as_uuid());
    let projection = fixture
        .projections
        .find(fixture.assignment.organization_id, projection_id)
        .await
        .expect("load")
        .expect("projection");
    assert_eq!(projection.plan_digest.as_str(), planned.plan_digest);

    let stored_plan: StoredPlan =
        serde_json::from_value(stored["stored"].clone()).expect("stored");
    let confirm_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored_plan),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("confirm ready");
    assert_eq!(confirm_ready["state"], "ready");
    let confirmed: ConfirmedPlan =
        serde_json::from_value(confirm_ready["confirmed"].clone()).expect("confirmed");
    let apply_input = EnqueueApplyInput {
        confirmed: Box::new(confirmed),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply pending");
    let apply_command_id = enqueue_apply_command_id(fixture.operation_id.as_uuid());
    acknowledge_apply(&fixture, apply_command_id)
        .await
        .expect("disable apply ack");
    let apply_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply ready");
    assert_eq!(apply_ready["state"], "ready");
    assert_eq!(apply_ready["applied"]["packageGeneration"], 13);

    let applied: AppliedAssignment =
        serde_json::from_value(apply_ready["applied"].clone()).expect("applied");
    let observe_input = ObserveInput {
        applied: Box::new(applied),
        observation_attempt: 1,
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe pending");
    acknowledge_observe_with_state(
        &fixture,
        observe_command_id(fixture.operation_id.as_uuid(), 1),
        PluginHostPackageState {
            version: Some(fixture.assignment.selection.version.clone()),
            package_generation: Some(13),
            package_digest: Some(fixture.assignment.selection.package_digest.as_str().into()),
            manifest_digest: Some(fixture.assignment.selection.manifest_digest.as_str().into()),
            receipt_digest: Some(digest('b').as_str().into()),
            capability_generation: 14,
            capability_revision: digest('c').as_str().into(),
            desired: PluginDesiredState::InstalledDisabled,
            observed: PluginObservedState::Installed,
            selected_surfaces: fixture.assignment.selection.selected_surfaces.clone(),
        },
    )
    .await
    .expect("disable observe ack");
    let observe_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe ready");
    assert_eq!(observe_ready["state"], "ready");
}

#[tokio::test]
async fn store_plan_persists_projection_from_planned_ack() {
    let fixture = Fixture::create().await;
    let plan_input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(&fixture)),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue pending observe");
    let command_id = ack_pre_plan_then_package_plan(&fixture)
        .await
        .expect("pre-plan ack");
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue pending plan");
    acknowledge_plan(&fixture, command_id)
        .await
        .expect("ack");
    let plan_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("plan ready");
    let planned: PlannedAssignment =
        serde_json::from_value(plan_ready["planned"].clone()).expect("planned");
    let store_input = StorePlanInput {
        planned: Box::new(planned.clone()),
    };
    let stored = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "store-plan",
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            serde_json::to_value(&store_input).expect("input"),
            vec![],
        ))
        .await
        .expect("store ready");
    assert_eq!(stored["state"], "ready");
    let projection_id = store_plan_projection_id(fixture.operation_id.as_uuid());
    assert_eq!(
        stored["stored"]["projectionId"],
        projection_id.as_uuid().to_string()
    );
    assert_eq!(
        stored["stored"]["planDigest"],
        planned.plan_digest
    );
    assert_eq!(stored["stored"]["awaitsConfirmation"], false);
    let projection = fixture
        .projections
        .find(fixture.assignment.organization_id, projection_id)
        .await
        .expect("load projection")
        .expect("projection exists");
    assert_eq!(projection.plan_digest.as_str(), planned.plan_digest);
    assert_eq!(projection.assignment_id, fixture.assignment.id);
    assert_eq!(projection.operation_id, fixture.operation_id);

    let replayed = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "store-plan",
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            serde_json::to_value(&store_input).expect("input"),
            vec![],
        ))
        .await
        .expect("store idempotent");
    assert_eq!(replayed["state"], "ready");
    assert_eq!(
        replayed["stored"]["projectionId"],
        projection_id.as_uuid().to_string()
    );
}

async fn store_ready_plan(
    fixture: &Fixture,
    decision: PlanPolicyDecision,
) -> (StoredPlan, serde_json::Value) {
    let plan_input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(fixture)),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue pending observe");
    let command_id = ack_pre_plan_then_package_plan(fixture)
        .await
        .expect("pre-plan ack");
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue pending plan");
    acknowledge_plan_with_decision(fixture, command_id, decision)
        .await
        .expect("ack");
    let plan_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("plan ready");
    let planned: PlannedAssignment =
        serde_json::from_value(plan_ready["planned"].clone()).expect("planned");
    let store_input = StorePlanInput {
        planned: Box::new(planned),
    };
    let store_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "store-plan",
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            serde_json::to_value(&store_input).expect("input"),
            vec![],
        ))
        .await
        .expect("store ready");
    let stored: StoredPlan =
        serde_json::from_value(store_ready["stored"].clone()).expect("stored");
    (stored, store_ready)
}

#[tokio::test]
async fn crash_point_2_plan_command_survives_restart_before_ack() {
    let fixture = Fixture::create().await;
    let input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(&fixture)),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue pre-plan observe");
    let plan_command_id = ack_pre_plan_then_package_plan(&fixture)
        .await
        .expect("pre-plan present");
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("plan pending");
    assert_eq!(pending["state"], "pending");
    let first = fixture
        .nodes
        .find_command(fixture.node_id, plan_command_id)
        .await
        .expect("load")
        .expect("plan command");

    let pending_again = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("plan pending after restart");
    assert_eq!(pending_again["state"], "pending");
    let second = fixture
        .nodes
        .find_command(fixture.node_id, plan_command_id)
        .await
        .expect("reload")
        .expect("same plan command");
    assert_eq!(second.id, first.id);
    assert_eq!(second.payload_digest(), first.payload_digest());
    assert_eq!(second.issued_at, first.issued_at);
}

#[tokio::test]
async fn crash_point_3_confirmation_resumes_from_stored_projection() {
    let fixture = Fixture::create().await;
    let (stored, store_ready) = store_ready_plan(&fixture, PlanPolicyDecision::Allow).await;
    let projection_id = store_plan_projection_id(fixture.operation_id.as_uuid());
    let before = fixture
        .projections
        .find(fixture.assignment.organization_id, projection_id)
        .await
        .expect("load")
        .expect("projection");

    let replayed_store = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "store-plan",
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            serde_json::to_value(StorePlanInput {
                planned: stored.planned.clone(),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("store after restart");
    assert_eq!(replayed_store["state"], "ready");
    assert_eq!(
        replayed_store["stored"]["projectionId"],
        store_ready["stored"]["projectionId"]
    );

    let confirm = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("confirm after restart");
    assert_eq!(confirm["state"], "ready");
    let after = fixture
        .projections
        .find(fixture.assignment.organization_id, projection_id)
        .await
        .expect("reload")
        .expect("projection still present");
    assert_eq!(after.plan_digest, before.plan_digest);
    assert_eq!(after.assignment_generation, before.assignment_generation);
}

#[tokio::test]
async fn crash_point_4_5_apply_command_survives_restart_before_ack() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Allow).await;
    let confirm_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("confirm ready");
    let confirmed: ConfirmedPlan =
        serde_json::from_value(confirm_ready["confirmed"].clone()).expect("confirmed");
    let apply_input = EnqueueApplyInput {
        confirmed: Box::new(confirmed),
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply pending");
    assert_eq!(pending["state"], "pending");
    let apply_command_id = enqueue_apply_command_id(fixture.operation_id.as_uuid());
    let first = fixture
        .nodes
        .find_command(fixture.node_id, apply_command_id)
        .await
        .expect("load")
        .expect("apply command");

    let pending_again = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply pending after restart");
    assert_eq!(pending_again["state"], "pending");
    let second = fixture
        .nodes
        .find_command(fixture.node_id, apply_command_id)
        .await
        .expect("reload")
        .expect("same apply command");
    assert_eq!(second.id, first.id);
    assert_eq!(second.payload_digest(), first.payload_digest());
    assert_eq!(second.issued_at, first.issued_at);
}

#[tokio::test]
async fn crash_point_8_9_observe_command_survives_restart_before_ack() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Allow).await;
    let confirm_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("confirm ready");
    let confirmed: ConfirmedPlan =
        serde_json::from_value(confirm_ready["confirmed"].clone()).expect("confirmed");
    let apply_input = EnqueueApplyInput {
        confirmed: Box::new(confirmed),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply pending");
    acknowledge_apply(
        &fixture,
        enqueue_apply_command_id(fixture.operation_id.as_uuid()),
    )
    .await
    .expect("apply ack");
    let apply_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply ready");
    let applied: AppliedAssignment =
        serde_json::from_value(apply_ready["applied"].clone()).expect("applied");
    let observe_input = ObserveInput {
        applied: Box::new(applied),
        observation_attempt: 1,
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe pending");
    assert_eq!(pending["state"], "pending");
    let observe_id = observe_command_id(fixture.operation_id.as_uuid(), 1);
    let first = fixture
        .nodes
        .find_command(fixture.node_id, observe_id)
        .await
        .expect("load")
        .expect("observe command");

    let pending_again = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe pending after restart");
    assert_eq!(pending_again["state"], "pending");
    let second = fixture
        .nodes
        .find_command(fixture.node_id, observe_id)
        .await
        .expect("reload")
        .expect("same observe command");
    assert_eq!(second.id, first.id);
    assert_eq!(second.payload_digest(), first.payload_digest());
    assert_eq!(second.issued_at, first.issued_at);

    acknowledge_observe(&fixture, observe_id)
        .await
        .expect("observe ack");
    let observe_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe ready after ack");
    assert_eq!(observe_ready["state"], "ready");
}

#[tokio::test]
async fn await_confirmation_allows_without_human_digest() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Allow).await;
    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored.clone()),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("allow ready");
    assert_eq!(ready["state"], "ready");
    assert!(ready["confirmed"]["confirmationDigest"].is_null());
    assert_eq!(
        ready["confirmed"]["stored"]["projectionId"],
        stored.projection_id.as_uuid().to_string()
    );
}

#[tokio::test]
async fn await_confirmation_asks_until_digest_is_recorded() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Ask).await;
    assert!(stored.awaits_confirmation);
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored.clone()),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("ask pending");
    assert_eq!(pending["state"], "pending");

    let projection = fixture
        .projections
        .find(fixture.assignment.organization_id, stored.projection_id)
        .await
        .expect("load")
        .expect("projection");
    let confirmed_at = Utc::now();
    let confirmation = PluginOperationConfirmation {
        schema: PLUGIN_OPERATION_CONFIRMATION_SCHEMA.into(),
        operation_id: stored.planned.use_operation_id.clone(),
        plan_digest: stored.plan_digest.clone(),
        confirmed_by: PlanActor::User,
        confirmed_at_ms: u64::try_from(confirmed_at.timestamp_millis()).expect("ms"),
    };
    let confirmed = projection
        .confirm(&confirmation, confirmed_at)
        .expect("confirm");
    fixture
        .projections
        .update(confirmed.clone(), None)
        .await
        .expect("persist confirmation");

    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("ask ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["confirmed"]["confirmationDigest"],
        confirmed.confirmation_digest.as_ref().unwrap().as_str()
    );
    assert!(ready["confirmed"]["confirmation"].is_object());
}

#[tokio::test]
async fn await_confirmation_denies_without_enqueueing_apply() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Deny).await;
    let terminal = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("deny terminal");
    assert_eq!(terminal["state"], "terminal");
    assert!(terminal["reason"]
        .as_str()
        .unwrap()
        .contains("denied by policy"));
    let apply_command = fixture
        .nodes
        .find_command(
            fixture.node_id,
            enqueue_apply_command_id(fixture.operation_id.as_uuid()),
        )
        .await
        .expect("find apply");
    assert!(apply_command.is_none());
}

#[tokio::test]
async fn await_confirmation_ask_times_out_when_plan_expires() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Ask).await;
    let mut projection = fixture
        .projections
        .find(fixture.assignment.organization_id, stored.projection_id)
        .await
        .expect("load")
        .expect("projection");
    projection.expires_at = Utc::now() - Duration::seconds(5);
    fixture
        .projections
        .update(projection, None)
        .await
        .expect("expire projection");
    let terminal = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("timeout terminal");
    assert_eq!(terminal["state"], "terminal");
    assert!(terminal["reason"]
        .as_str()
        .unwrap()
        .contains("timed out"));
}

#[tokio::test]
async fn enqueue_plan_short_circuits_when_already_converged() {
    let fixture = Fixture::create().await;
    let input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(&fixture)),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pre-plan pending");
    let observe_id = pre_plan_observe_command_id(fixture.operation_id.as_uuid());
    acknowledge_observe_with_state(&fixture, observe_id, installed_enabled_state(&fixture))
        .await
        .expect("pre-plan converged ack");
    let converged = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("already converged");
    assert_eq!(converged["state"], "already_converged");
    assert_eq!(converged["converged"]["observedDesired"], "enabled");
    assert_eq!(converged["converged"]["observedState"], "ready");
    let plan_command = fixture
        .nodes
        .find_command(
            fixture.node_id,
            enqueue_plan_command_id(fixture.operation_id.as_uuid()),
        )
        .await
        .expect("find plan");
    assert!(plan_command.is_none());
}

#[tokio::test]
async fn enqueue_uninstall_plan_when_desired_absent_and_converges_to_removed() {
    let fixture = Fixture::create_with_desired(PluginDesiredState::Absent).await;
    let input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(&fixture)),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pre-plan pending");
    let command_id = ack_pre_plan_then_package_plan_with_state(
        &fixture,
        installed_enabled_state(&fixture),
    )
    .await
    .expect("pre-plan present");
    let pending_plan = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("uninstall plan pending");
    assert_eq!(pending_plan["state"], "pending");
    acknowledge_plan(&fixture, command_id)
        .await
        .expect("uninstall plan ack");
    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("uninstall plan ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["planned"]["action"], "uninstall");
    let planned: PlannedAssignment =
        serde_json::from_value(ready["planned"].clone()).expect("planned");
    let stored = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "store-plan",
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            serde_json::to_value(&StorePlanInput {
                planned: Box::new(planned),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("store uninstall");
    let stored_plan: StoredPlan =
        serde_json::from_value(stored["stored"].clone()).expect("stored");
    let confirm_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored_plan),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("confirm");
    let confirmed: ConfirmedPlan =
        serde_json::from_value(confirm_ready["confirmed"].clone()).expect("confirmed");
    let apply_input = EnqueueApplyInput {
        confirmed: Box::new(confirmed),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply pending");
    acknowledge_apply(
        &fixture,
        enqueue_apply_command_id(fixture.operation_id.as_uuid()),
    )
    .await
    .expect("uninstall apply");
    let apply_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply ready");
    let applied: AppliedAssignment =
        serde_json::from_value(apply_ready["applied"].clone()).expect("applied");
    let observe_input = ObserveInput {
        applied: Box::new(applied),
        observation_attempt: 1,
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe pending");
    acknowledge_observe_with_state(
        &fixture,
        observe_command_id(fixture.operation_id.as_uuid(), 1),
        PluginHostPackageState {
            version: None,
            package_generation: None,
            package_digest: None,
            manifest_digest: None,
            receipt_digest: None,
            capability_generation: 15,
            capability_revision: digest('c').as_str().into(),
            desired: PluginDesiredState::Absent,
            observed: PluginObservedState::Removed,
            selected_surfaces: Vec::new(),
        },
    )
    .await
    .expect("removed observe");
    let observe_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe ready");
    assert_eq!(observe_ready["state"], "ready");
}

#[tokio::test]
async fn enqueue_upgrade_plan_when_observed_package_drifts_and_converges() {
    let fixture = Fixture::create().await;
    let input = EnqueuePlanInput {
        authorized: Box::new(authorized_trust(&fixture)),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("pre-plan pending");
    let command_id =
        ack_pre_plan_then_package_plan_with_state(&fixture, drifted_installed_state(&fixture))
            .await
            .expect("drifted pre-plan");
    let pending_plan = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("upgrade pending");
    assert_eq!(pending_plan["state"], "pending");
    // Upgrade envelopes require prior+candidate package locks owned by Use.
    // Cloud's mutation matrix proves selection + Fleet plan enqueue identity;
    // full upgrade apply/observe stays on the Use host converge gate.
    let (lease_id, command) = lease_command(&fixture, command_id)
        .await
        .expect("lease upgrade plan");
    let NodeCommandPayload::PluginHostPlan { request } = command.payload.clone() else {
        panic!("expected package plan");
    };
    assert_eq!(request.action, PluginOperationAction::Upgrade);
    assert!(request.candidate.is_some());
    fixture
        .nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: command.command_id,
                lease_id,
                node_id: fixture.node_id.as_uuid(),
                sequence: command.sequence,
                payload_digest: command.payload_digest.clone(),
                completed_at: command.issued_at + Duration::milliseconds(50),
                outcome: NodeCommandOutcome::Failed {
                    failure: a3s_cloud_contracts::NodeCommandFailure {
                        code: "use.plugin.upgrade_lock_required".into(),
                        message: "upgrade package locks are owned by A3S Use".into(),
                        retryable: false,
                    },
                },
            },
            command.issued_at + Duration::milliseconds(50),
        )
        .await
        .expect("ack upgrade identity");
    let terminal = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&input).expect("input"),
            vec![],
        ))
        .await
        .expect("upgrade terminal without locks");
    assert_eq!(terminal["state"], "terminal");
}

#[tokio::test]
async fn enqueue_apply_accepts_exact_ack_for_allow_plan() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Allow).await;
    let confirm_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("confirm ready");
    let confirmed: ConfirmedPlan =
        serde_json::from_value(confirm_ready["confirmed"].clone()).expect("confirmed");
    let apply_input = EnqueueApplyInput {
        confirmed: Box::new(confirmed),
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply pending");
    assert_eq!(pending["state"], "pending");

    let command_id = enqueue_apply_command_id(fixture.operation_id.as_uuid());
    acknowledge_apply(&fixture, command_id)
        .await
        .expect("apply ack");
    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["applied"]["commandId"],
        command_id.as_uuid().to_string()
    );
    assert_eq!(ready["applied"]["capabilityGeneration"], 14);
    assert_eq!(ready["applied"]["packageGeneration"], 13);
}

#[tokio::test]
async fn observe_accepts_exact_ack_for_converged_package() {
    let fixture = Fixture::create().await;
    let (stored, _) = store_ready_plan(&fixture, PlanPolicyDecision::Allow).await;
    let confirm_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("confirm ready");
    let confirmed: ConfirmedPlan =
        serde_json::from_value(confirm_ready["confirmed"].clone()).expect("confirmed");
    let apply_input = EnqueueApplyInput {
        confirmed: Box::new(confirmed),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply pending");
    acknowledge_apply(
        &fixture,
        enqueue_apply_command_id(fixture.operation_id.as_uuid()),
    )
    .await
    .expect("apply ack");
    let apply_ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply ready");
    let applied: AppliedAssignment =
        serde_json::from_value(apply_ready["applied"].clone()).expect("applied");
    let observe_input = ObserveInput {
        applied: Box::new(applied),
        observation_attempt: 1,
    };
    let pending = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe pending");
    assert_eq!(pending["state"], "pending");

    let command_id = observe_command_id(fixture.operation_id.as_uuid(), 1);
    acknowledge_observe(&fixture, command_id)
        .await
        .expect("observe ack");
    let ready = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe ready");
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["observed"]["commandId"],
        command_id.as_uuid().to_string()
    );
    assert_eq!(ready["observed"]["observedDesired"], "enabled");
    assert_eq!(ready["observed"]["observedState"], "ready");
    assert_eq!(ready["observed"]["capabilityGeneration"], 14);
    assert_eq!(ready["observed"]["packageGeneration"], 13);
}

#[tokio::test]
async fn workflow_completes_after_observe_ready() {
    let fixture = Fixture::create().await;
    let input = fixture.flow_input();
    let first = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            vec![],
        ))
        .await
        .expect("schedule lock");
    let RuntimeCommand::ScheduleStep { step_name, .. } = first else {
        panic!("expected lock ScheduleStep, got {first:?}");
    };
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_LOCK);

    let lock_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "lock",
            PLUGIN_ASSIGNMENT_LOCK,
            input.clone(),
            vec![],
        ))
        .await
        .expect("lock");
    let after_lock = vec![FlowEventEnvelope::new(
        "run-1",
        1,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "lock".into(),
            output: lock_output,
        },
    )];
    let second = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            after_lock.clone(),
        ))
        .await
        .expect("schedule resolve");
    let RuntimeCommand::ScheduleStep {
        step_id,
        step_name,
        ..
    } = second
    else {
        panic!("expected resolve-host ScheduleStep, got {second:?}");
    };
    assert_eq!(step_id, "resolve-host-1");
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_RESOLVE_HOST);

    let resolve_input = ResolveHostInput {
        locked: Box::new(fixture.locked()),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "resolve-host-1",
            PLUGIN_ASSIGNMENT_RESOLVE_HOST,
            serde_json::to_value(&resolve_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue inspect");
    acknowledge_capabilities(
        &fixture,
        NodeCommandId::from_uuid(fixture.operation_id.as_uuid()),
    )
    .await
    .expect("ack");
    let resolve_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "resolve-host-1",
            PLUGIN_ASSIGNMENT_RESOLVE_HOST,
            serde_json::to_value(&resolve_input).expect("input"),
            vec![],
        ))
        .await
        .expect("resolve ready");
    let mut history = after_lock;
    history.push(FlowEventEnvelope::new(
        "run-1",
        2,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "resolve-host-1".into(),
            output: resolve_output.clone(),
        },
    ));
    let third = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            history.clone(),
        ))
        .await
        .expect("schedule authorize trust");
    let RuntimeCommand::ScheduleStep {
        step_id,
        step_name,
        ..
    } = third
    else {
        panic!("expected authorize-trust ScheduleStep, got {third:?}");
    };
    assert_eq!(step_id, "authorize-trust-1");
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST);

    let resolved: ResolvedHost =
        serde_json::from_value(resolve_output["resolved"].clone()).expect("resolved");
    let authorize_input = AuthorizeTrustInput {
        resolved: Box::new(resolved),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "authorize-trust-1",
            PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
            serde_json::to_value(&authorize_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue authorize");
    acknowledge_authorize_trust(
        &fixture,
        authorize_trust_command_id(fixture.operation_id.as_uuid()),
    )
    .await
    .expect("authorize ack");
    let authorize_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "authorize-trust-1",
            PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
            serde_json::to_value(&authorize_input).expect("input"),
            vec![],
        ))
        .await
        .expect("authorize ready");
    history.push(FlowEventEnvelope::new(
        "run-1",
        3,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "authorize-trust-1".into(),
            output: authorize_output.clone(),
        },
    ));
    let fourth = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            history.clone(),
        ))
        .await
        .expect("schedule enqueue plan");
    let RuntimeCommand::ScheduleStep {
        step_id,
        step_name,
        ..
    } = fourth
    else {
        panic!("expected enqueue-plan ScheduleStep, got {fourth:?}");
    };
    assert_eq!(step_id, "enqueue-plan-1");
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_ENQUEUE_PLAN);

    let authorized: AuthorizedTrust =
        serde_json::from_value(authorize_output["authorized"].clone()).expect("authorized");
    let plan_input = EnqueuePlanInput {
        authorized: Box::new(authorized),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue pre-plan observe");
    let plan_command_id = ack_pre_plan_then_package_plan(&fixture)
        .await
        .expect("pre-plan ack");
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue package plan");
    acknowledge_plan(&fixture, plan_command_id)
        .await
        .expect("plan ack");
    let plan_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-plan-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
            serde_json::to_value(&plan_input).expect("input"),
            vec![],
        ))
        .await
        .expect("plan ready");
    history.push(FlowEventEnvelope::new(
        "run-1",
        4,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "enqueue-plan-1".into(),
            output: plan_output.clone(),
        },
    ));
    let fifth = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            history.clone(),
        ))
        .await
        .expect("schedule store plan");
    let RuntimeCommand::ScheduleStep {
        step_id,
        step_name,
        ..
    } = fifth
    else {
        panic!("expected store-plan ScheduleStep, got {fifth:?}");
    };
    assert_eq!(step_id, "store-plan");
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_STORE_PLAN);

    let planned: PlannedAssignment =
        serde_json::from_value(plan_output["planned"].clone()).expect("planned");
    let store_input = StorePlanInput {
        planned: Box::new(planned),
    };
    let store_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "store-plan",
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            serde_json::to_value(&store_input).expect("input"),
            vec![],
        ))
        .await
        .expect("store plan");
    history.push(FlowEventEnvelope::new(
        "run-1",
        5,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "store-plan".into(),
            output: store_output.clone(),
        },
    ));
    let sixth = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            history.clone(),
        ))
        .await
        .expect("schedule await confirmation");
    let RuntimeCommand::ScheduleStep {
        step_id,
        step_name,
        ..
    } = sixth
    else {
        panic!("expected await-confirmation ScheduleStep, got {sixth:?}");
    };
    assert_eq!(step_id, "await-confirmation-1");
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION);

    let stored: StoredPlan =
        serde_json::from_value(store_output["stored"].clone()).expect("stored");
    let confirm_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "await-confirmation-1",
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
            serde_json::to_value(AwaitConfirmationInput {
                stored: Box::new(stored),
            })
            .expect("input"),
            vec![],
        ))
        .await
        .expect("await confirmation");
    history.push(FlowEventEnvelope::new(
        "run-1",
        6,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "await-confirmation-1".into(),
            output: confirm_output.clone(),
        },
    ));
    let seventh = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            history.clone(),
        ))
        .await
        .expect("schedule enqueue apply");
    let RuntimeCommand::ScheduleStep {
        step_id,
        step_name,
        ..
    } = seventh
    else {
        panic!("expected enqueue-apply ScheduleStep, got {seventh:?}");
    };
    assert_eq!(step_id, "enqueue-apply-1");
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_ENQUEUE_APPLY);

    let confirmed: ConfirmedPlan =
        serde_json::from_value(confirm_output["confirmed"].clone()).expect("confirmed");
    let apply_input = EnqueueApplyInput {
        confirmed: Box::new(confirmed),
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue apply");
    acknowledge_apply(
        &fixture,
        enqueue_apply_command_id(fixture.operation_id.as_uuid()),
    )
    .await
    .expect("apply ack");
    let apply_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "enqueue-apply-1",
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
            serde_json::to_value(&apply_input).expect("input"),
            vec![],
        ))
        .await
        .expect("apply ready");
    history.push(FlowEventEnvelope::new(
        "run-1",
        7,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "enqueue-apply-1".into(),
            output: apply_output.clone(),
        },
    ));
    let eighth = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input.clone(),
            history.clone(),
        ))
        .await
        .expect("schedule observe");
    let RuntimeCommand::ScheduleStep {
        step_id,
        step_name,
        ..
    } = eighth
    else {
        panic!("expected observe ScheduleStep, got {eighth:?}");
    };
    assert_eq!(step_id, "observe-1");
    assert_eq!(step_name, PLUGIN_ASSIGNMENT_OBSERVE);

    let applied: AppliedAssignment =
        serde_json::from_value(apply_output["applied"].clone()).expect("applied");
    let observe_input = ObserveInput {
        applied: Box::new(applied),
        observation_attempt: 1,
    };
    let _ = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("enqueue observe");
    acknowledge_observe(
        &fixture,
        observe_command_id(fixture.operation_id.as_uuid(), 1),
    )
    .await
    .expect("observe ack");
    let observe_output = fixture
        .runtime
        .run_step(StepInvocation::new(
            "run-1",
            "observe-1",
            PLUGIN_ASSIGNMENT_OBSERVE,
            serde_json::to_value(&observe_input).expect("input"),
            vec![],
        ))
        .await
        .expect("observe ready");
    history.push(FlowEventEnvelope::new(
        "run-1",
        8,
        Uuid::now_v7(),
        Utc::now(),
        FlowEvent::StepCompleted {
            step_id: "observe-1".into(),
            output: observe_output.clone(),
        },
    ));
    let completed = fixture
        .runtime
        .run_workflow(WorkflowInvocation::new(
            "run-1",
            workflow_spec(),
            input,
            history,
        ))
        .await
        .expect("complete after observe");
    let RuntimeCommand::Complete { output, .. } = completed else {
        panic!("expected Complete after observe Ready, got {completed:?}");
    };
    assert_eq!(output["observedDesired"], "enabled");
    assert_eq!(output["observedState"], "ready");
    assert_eq!(output["capabilityGeneration"], 14);
    assert_eq!(output["packageGeneration"], 13);
    assert_eq!(observe_output["state"], "ready");
    assert_eq!(apply_output["state"], "ready");
}

#[test]
fn registered_identities_and_steps_are_stable() {
    let identities: Vec<_> = super::flow_workflow_identities().collect();
    assert_eq!(
        identities,
        vec![(
            PLUGIN_ASSIGNMENT_WORKFLOW_NAME,
            PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
        )]
    );
    let steps: Vec<_> = super::flow_step_names().collect();
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_LOCK));
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_RESOLVE_HOST));
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST));
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_ENQUEUE_PLAN));
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_STORE_PLAN));
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION));
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_ENQUEUE_APPLY));
    assert!(steps.contains(&PLUGIN_ASSIGNMENT_OBSERVE));
    assert_eq!(steps.len(), 8);
}
