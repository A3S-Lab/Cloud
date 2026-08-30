use super::*;
use crate::modules::durable_cells::application::{
    admit_durable_cell_operator_observation, admit_durable_cell_runtime_apply,
    admit_durable_cell_runtime_remove, admit_durable_cell_runtime_stop,
    project_durable_cell_operator_binding, project_durable_cell_provider_workload,
    project_durable_cell_runtime_spec,
};
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec, DurableCellClassSpec,
    DurableCellRollbackPolicy, DurableCellServiceProfileSpec, DurableCellStateSchema,
};
use crate::modules::shared_kernel::domain::{
    BuildRunId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    OrganizationId, PrincipalId, ProjectId, ResourceName,
};
use crate::modules::workloads::{
    HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    WorkloadRevision,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, NodeDurableCellOperatorObservationV1,
};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeEvidence, RuntimeHealthObservation,
    RuntimeHealthState, RuntimeInspection, RuntimeObservation, RuntimeRemoval,
    RuntimeServiceEndpoint, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
};
use chrono::{Duration, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

struct Fixture {
    application: DurableCellApplication,
    application_revision: DurableCellApplicationRevision,
    projection: DurableCellProjectionIdentity,
    profile: DurableCellServiceProfile,
    workload_revision: WorkloadRevision,
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn profile() -> DurableCellServiceProfile {
    DurableCellServiceProfile::from_spec(DurableCellServiceProfileSpec {
        public_runtime_port: "cell-public".into(),
        internal_runtime_port: "cell-internal".into(),
        health_path: "/__a3s/cell/health".into(),
        max_cell_name_bytes: 512,
        max_request_bytes: 16 * 1024 * 1024,
        max_response_bytes: 64 * 1024 * 1024,
        max_websocket_message_bytes: 1024 * 1024,
    })
    .expect("profile")
}

fn service_template(profile: &DurableCellServiceProfile) -> ServiceTemplate {
    let artifact_digest = digest('d');
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!(
                "oci://registry.example/a3s/cell-provider@{}",
                artifact_digest.as_str()
            ),
            digest: artifact_digest.to_string(),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/app/a3s-cell-provider".into()],
            args: vec!["serve".into()],
            working_directory: Some("/app".into()),
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 500,
            memory_bytes: 256 * 1024 * 1024,
            pids: 128,
            ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
        },
        ports: vec![
            ServicePort {
                name: profile.spec().public_runtime_port.clone(),
                container_port: 8080,
            },
            ServicePort {
                name: profile.spec().internal_runtime_port.clone(),
                container_port: 9090,
            },
        ],
        health: Some(HttpHealthCheck {
            port_name: profile.spec().public_runtime_port.clone(),
            path: profile.spec().health_path.clone(),
            interval_ms: 5_000,
            timeout_ms: 1_000,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 10_000,
        }),
    }
}

fn fixture_with_template(
    make_template: impl FnOnce(&DurableCellServiceProfile) -> ServiceTemplate,
) -> Fixture {
    let profile = profile();
    let application_id = DurableCellApplicationId::new();
    let definition =
        DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
            build_run_id: BuildRunId::new(),
            bundle_digest: digest('a'),
            bundle_size_bytes: 4096,
            main_module: "worker.mjs".into(),
            compatibility_date: "2026-08-16".into(),
            compatibility_flags: Vec::new(),
            cell_classes: vec![DurableCellClassSpec {
                name: "Counter".into(),
                state_schema: DurableCellStateSchema {
                    minimum_readable_version: 1,
                    maximum_readable_version: 1,
                    write_version: 1,
                },
            }],
            service_profile_digest: profile.digest().clone(),
            rollback_policy: DurableCellRollbackPolicy::Compatible,
        })
        .expect("definition");
    let application_revision = DurableCellApplicationRevision::initial(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        application_id,
        DurableCellApplicationRevisionId::new(),
        definition,
        PrincipalId::new(),
        Utc::now(),
    )
    .expect("application revision");
    let application = DurableCellApplication::create(
        application_id,
        ResourceName::parse("Counters").expect("name"),
        &application_revision,
    )
    .expect("application");
    let projection =
        DurableCellProjectionIdentity::for_current_revision(&application, &application_revision)
            .expect("projection");
    let workload_revision = WorkloadRevision::create(
        projection.workload_revision_id,
        projection.workload_id,
        application_revision.revision_number,
        make_template(&profile),
        application_revision.created_at,
    )
    .expect("Workload revision");
    Fixture {
        application,
        application_revision,
        projection,
        profile,
        workload_revision,
    }
}

fn fixture() -> Fixture {
    fixture_with_template(service_template)
}

fn binding(fixture: &Fixture) -> DurableCellProviderBinding {
    let workload = project_durable_cell_provider_workload(&fixture.workload_revision)
        .expect("provider Workload projection");
    DurableCellProviderBinding::for_current_revision(
        &fixture.application,
        &fixture.application_revision,
        &fixture.projection,
        &fixture.profile,
        &workload,
    )
    .expect("provider binding")
}

#[test]
fn provider_selection_binds_one_existing_digest_pinned_workload_revision() {
    let fixture = fixture();
    let workload = project_durable_cell_provider_workload(&fixture.workload_revision)
        .expect("provider Workload projection");
    let binding = binding(&fixture);
    assert_eq!(binding.application_id, fixture.application.id);
    assert_eq!(binding.workload_id, fixture.projection.workload_id);
    assert_eq!(
        binding.workload_revision_id,
        fixture.projection.workload_revision_id
    );
    assert_eq!(binding.service_profile_digest, *fixture.profile.digest());
    assert_eq!(
        binding.provider_artifact_digest.as_str(),
        fixture
            .workload_revision
            .resolved_template()
            .expect("template")
            .artifact
            .digest
    );
    binding
        .clone()
        .restore(
            &fixture.application,
            &fixture.application_revision,
            &fixture.projection,
            &fixture.profile,
            &workload,
        )
        .expect("restored binding");

    let mut drifted = binding;
    drifted.provider_artifact_digest = digest('e');
    assert!(drifted
        .restore(
            &fixture.application,
            &fixture.application_revision,
            &fixture.projection,
            &fixture.profile,
            &workload,
        )
        .is_err());
}

#[test]
fn provider_template_rejects_extra_surface_shared_socket_or_internal_health() {
    let extra = fixture_with_template(|profile| {
        let mut template = service_template(profile);
        template.ports.push(ServicePort {
            name: "debug".into(),
            container_port: 9191,
        });
        template
    });
    assert!(DurableCellProviderBinding::for_current_revision(
        &extra.application,
        &extra.application_revision,
        &extra.projection,
        &extra.profile,
        &project_durable_cell_provider_workload(&extra.workload_revision)
            .expect("provider Workload projection"),
    )
    .is_err());

    let shared = fixture_with_template(|profile| {
        let mut template = service_template(profile);
        template.ports[1].container_port = template.ports[0].container_port;
        template
    });
    assert!(DurableCellProviderBinding::for_current_revision(
        &shared.application,
        &shared.application_revision,
        &shared.projection,
        &shared.profile,
        &project_durable_cell_provider_workload(&shared.workload_revision)
            .expect("provider Workload projection"),
    )
    .is_err());

    let internal_health = fixture_with_template(|profile| {
        let mut template = service_template(profile);
        template.health.as_mut().expect("health").port_name =
            profile.spec().internal_runtime_port.clone();
        template
    });
    assert!(DurableCellProviderBinding::for_current_revision(
        &internal_health.application,
        &internal_health.application_revision,
        &internal_health.projection,
        &internal_health.profile,
        &project_durable_cell_provider_workload(&internal_health.workload_revision)
            .expect("provider Workload projection"),
    )
    .is_err());
}

#[test]
fn provider_projects_only_an_ordinary_profile_bound_runtime_service() {
    let fixture = fixture();
    let binding = binding(&fixture);
    let spec =
        project_durable_cell_runtime_spec(&binding, &fixture.profile, &fixture.workload_revision)
            .expect("Runtime Service");
    assert_eq!(spec.class, RuntimeUnitClass::Service);
    assert_eq!(
        spec.semantics_profile_digest.as_deref(),
        Some(fixture.profile.digest().as_str())
    );
    assert_eq!(spec.network.ports.len(), 2);
    assert!(spec.outputs.is_empty());
    let names = spec
        .network
        .ports
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["cell-internal", "cell-public"]));
}

#[test]
fn runtime_admission_consumes_the_exact_existing_fleet_receipt() {
    let fixture = fixture();
    let binding = binding(&fixture);
    let spec =
        project_durable_cell_runtime_spec(&binding, &fixture.profile, &fixture.workload_revision)
            .expect("Runtime Service");
    let observation = healthy_observation(&spec, RuntimeHealthState::Healthy);
    let (command, acknowledgement) = runtime_apply_receipt(spec, observation);
    let endpoints = admit_durable_cell_runtime_apply(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &command,
        &acknowledgement,
    )
    .expect("admitted Runtime receipt");
    assert_eq!(endpoints.public.port_name, "cell-public");
    assert_eq!(endpoints.internal.port_name, "cell-internal");
    assert_ne!(
        endpoints.public.socket_addr(),
        endpoints.internal.socket_addr()
    );

    let mut forged = acknowledgement.clone();
    forged.payload_digest = digest('f').to_string();
    assert!(admit_durable_cell_runtime_apply(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &command,
        &forged,
    )
    .is_err());

    let unhealthy = healthy_observation(
        match &command.payload {
            NodeCommandPayload::RuntimeApply { request, .. } => &request.spec,
            _ => unreachable!(),
        },
        RuntimeHealthState::Unhealthy,
    );
    let (unhealthy_command, unhealthy_ack) = runtime_apply_receipt(
        match &command.payload {
            NodeCommandPayload::RuntimeApply { request, .. } => request.spec.clone(),
            _ => unreachable!(),
        },
        unhealthy,
    );
    assert!(admit_durable_cell_runtime_apply(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &unhealthy_command,
        &unhealthy_ack,
    )
    .is_err());
}

#[test]
fn operator_observation_adopts_only_the_exact_healthy_runtime() {
    let fixture = fixture();
    let binding = binding(&fixture);
    let spec =
        project_durable_cell_runtime_spec(&binding, &fixture.profile, &fixture.workload_revision)
            .expect("Runtime Service");
    let apply_observation = healthy_observation(&spec, RuntimeHealthState::Healthy);
    let (apply_command, apply_acknowledgement) =
        runtime_apply_receipt(spec.clone(), apply_observation);
    let operator_binding = project_durable_cell_operator_binding(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
    )
    .expect("operator binding");
    assert_eq!(
        operator_binding.runtime_spec_digest,
        spec.digest().expect("digest")
    );
    assert_eq!(
        operator_binding.internal_service_port_name,
        fixture.profile.spec().internal_runtime_port
    );

    let issued_at = apply_acknowledgement.completed_at + Duration::milliseconds(1);
    let mut metadata = command_metadata(binding.application_id.as_uuid(), 2, issued_at);
    metadata.node_id = apply_command.node_id;
    let operator_command = NodeCommandEnvelope::new(
        metadata,
        NodeCommandPayload::DurableCellOperatorObserve {
            binding: Box::new(operator_binding.clone()),
        },
    )
    .expect("operator command");
    let observed_at_ms = u64::try_from(issued_at.timestamp_millis() + 1).expect("observation time");
    let observation = NodeDurableCellOperatorObservationV1 {
        schema: NodeDurableCellOperatorObservationV1::SCHEMA.into(),
        binding_digest: operator_binding.digest().expect("operator binding digest"),
        runtime_unit_id: operator_binding.runtime_unit_id.clone(),
        runtime_generation: operator_binding.runtime_generation,
        runtime_spec_digest: operator_binding.runtime_spec_digest.clone(),
        occupied: 3,
        evicting: 1,
        restoring: 2,
        activating: 1,
        activation_waiting: 4,
        capacity_waiting: 5,
        observed_at_ms,
    };
    let operator_acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: operator_command.command_id,
        lease_id: operator_command.lease_id,
        node_id: operator_command.node_id,
        sequence: operator_command.sequence,
        payload_digest: operator_command.payload_digest.clone(),
        completed_at: issued_at + Duration::milliseconds(2),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::DurableCellOperatorObserved {
                observation: observation.clone(),
            }),
        },
    };
    let admitted = admit_durable_cell_operator_observation(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &apply_command,
        &apply_acknowledgement,
        &operator_command,
        &operator_acknowledgement,
    )
    .expect("admitted operator observation");
    assert_eq!(admitted, observation);

    let foreign_node_id = Uuid::now_v7();
    let mut cross_node_command = operator_command.clone();
    cross_node_command.node_id = foreign_node_id;
    let mut cross_node_acknowledgement = operator_acknowledgement.clone();
    cross_node_acknowledgement.node_id = foreign_node_id;
    assert!(admit_durable_cell_operator_observation(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &apply_command,
        &apply_acknowledgement,
        &cross_node_command,
        &cross_node_acknowledgement,
    )
    .is_err());

    let mut forged = operator_acknowledgement;
    let NodeCommandOutcome::Succeeded { result } = &mut forged.outcome else {
        unreachable!()
    };
    let NodeCommandResult::DurableCellOperatorObserved { observation } = result.as_mut() else {
        unreachable!()
    };
    observation.runtime_spec_digest = digest('f').to_string();
    assert!(admit_durable_cell_operator_observation(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &apply_command,
        &apply_acknowledgement,
        &operator_command,
        &forged,
    )
    .is_err());
}

#[test]
fn drain_and_cleanup_admit_only_existing_runtime_receipts() {
    let fixture = fixture();
    let binding = binding(&fixture);
    let spec =
        project_durable_cell_runtime_spec(&binding, &fixture.profile, &fixture.workload_revision)
            .expect("Runtime Service");

    let (stop_command, stop_acknowledgement) = runtime_stop_receipt(&spec);
    assert_eq!(stop_command.payload.kind(), "runtime_stop");
    admit_durable_cell_runtime_stop(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &stop_command,
        &stop_acknowledgement,
    )
    .expect("Runtime stop evidence");

    let (remove_command, remove_acknowledgement) = runtime_remove_receipt(&spec);
    assert_eq!(remove_command.payload.kind(), "runtime_remove");
    admit_durable_cell_runtime_remove(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &remove_command,
        &remove_acknowledgement,
    )
    .expect("Runtime removal evidence");

    let mut stale = remove_acknowledgement;
    stale.schema = NodeCommandAck::LEGACY_SCHEMA.into();
    assert!(admit_durable_cell_runtime_remove(
        &binding,
        &fixture.profile,
        &fixture.workload_revision,
        &remove_command,
        &stale,
    )
    .is_err());
}

fn command_metadata(
    aggregate_id: Uuid,
    sequence: u64,
    issued_at: chrono::DateTime<Utc>,
) -> NodeCommandMetadata {
    NodeCommandMetadata {
        command_id: Uuid::now_v7(),
        lease_id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        sequence,
        aggregate_id,
        issued_at,
        not_after: issued_at + Duration::minutes(1),
        correlation_id: Uuid::now_v7(),
    }
}

fn runtime_stop_receipt(spec: &RuntimeUnitSpec) -> (NodeCommandEnvelope, NodeCommandAck) {
    let issued_at = Utc::now();
    let observed_at_ms = u64::try_from(issued_at.timestamp_millis() + 1).expect("stop time");
    let mut observation = healthy_observation(spec, RuntimeHealthState::Healthy);
    observation.clear_service_endpoints();
    observation.state = RuntimeUnitState::Stopped;
    observation.observed_at_ms = observed_at_ms;
    observation.started_at_ms = Some(observed_at_ms.saturating_sub(1));
    observation.finished_at_ms = Some(observed_at_ms);
    observation
        .validate_against(spec)
        .expect("stopped observation");
    let command = NodeCommandEnvelope::new(
        command_metadata(Uuid::now_v7(), 3, issued_at),
        NodeCommandPayload::RuntimeStop {
            request: RuntimeActionRequest {
                schema: RuntimeActionRequest::SCHEMA.into(),
                request_id: format!("durable-cell-stop:{}", Uuid::now_v7()),
                unit_id: spec.unit_id.clone(),
                generation: spec.generation,
                deadline_at_ms: None,
            },
        },
    )
    .expect("RuntimeStop command");
    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: issued_at + Duration::milliseconds(2),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeStopped {
                inspection: RuntimeInspection::Found {
                    schema: RuntimeInspection::SCHEMA.into(),
                    observation: Box::new(observation),
                },
            }),
        },
    };
    acknowledgement
        .validate_against(&command)
        .expect("RuntimeStop acknowledgement");
    (command, acknowledgement)
}

fn runtime_remove_receipt(spec: &RuntimeUnitSpec) -> (NodeCommandEnvelope, NodeCommandAck) {
    let issued_at = Utc::now();
    let removed_at_ms = u64::try_from(issued_at.timestamp_millis() + 1).expect("removal time");
    let request = RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("durable-cell-remove:{}", Uuid::now_v7()),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        deadline_at_ms: None,
    };
    let command = NodeCommandEnvelope::new(
        command_metadata(Uuid::now_v7(), 4, issued_at),
        NodeCommandPayload::RuntimeRemove {
            request: request.clone(),
        },
    )
    .expect("RuntimeRemove command");
    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: issued_at + Duration::milliseconds(2),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeRemoved {
                removal: RuntimeRemoval {
                    schema: RuntimeRemoval::SCHEMA.into(),
                    request_id: request.request_id,
                    unit_id: request.unit_id,
                    generation: request.generation,
                    removed_at_ms,
                    already_absent: false,
                },
            }),
        },
    };
    acknowledgement
        .validate_against(&command)
        .expect("RuntimeRemove acknowledgement");
    (command, acknowledgement)
}

fn healthy_observation(
    spec: &RuntimeUnitSpec,
    health_state: RuntimeHealthState,
) -> RuntimeObservation {
    let now_ms = u64::try_from(Utc::now().timestamp_millis()).expect("current timestamp");
    let spec_digest = spec.digest().expect("spec digest");
    let claims = spec
        .network
        .ports
        .iter()
        .enumerate()
        .map(|(index, port)| {
            let endpoint = RuntimeServiceEndpoint::node_local_tcp(
                &port.name,
                49_152 + u16::try_from(index).expect("port index"),
            )
            .expect("endpoint");
            (endpoint.claim_key(), endpoint.claim_value())
        })
        .collect();
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("cell-provider-fixture".into()),
        provider_build: Some("box-fixture".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: health_state,
            checked_at_ms: now_ms,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "box-fixture".into(),
            spec_digest,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            identity_attachment_digest: spec.identity_attachment_digest.clone(),
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation
        .validate_against(spec)
        .expect("Runtime observation");
    observation
}

fn runtime_apply_receipt(
    spec: RuntimeUnitSpec,
    observation: RuntimeObservation,
) -> (NodeCommandEnvelope, NodeCommandAck) {
    let issued_at = Utc::now();
    let command = NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            sequence: 1,
            aggregate_id: Uuid::now_v7(),
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("durable-cell-test:{}", Uuid::now_v7()),
                deadline_at_ms: None,
                spec,
            }),
            resource_claim: None,
        },
    )
    .expect("Fleet command");
    let acknowledgement = NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at: issued_at + Duration::seconds(1),
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(NodeCommandResult::RuntimeApplied {
                observation: Box::new(observation),
            }),
        },
    };
    acknowledgement
        .validate_against(&command)
        .expect("Fleet acknowledgement");
    (command, acknowledgement)
}
