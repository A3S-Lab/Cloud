use super::entities::*;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, DeploymentId, EnvironmentId, NodeCommandId, NodeId,
    OperationId, OrganizationId, ProjectId, ResourceName, SecretId, SourceRevisionId, WorkloadId,
    WorkloadRevisionId,
};
use a3s_cloud_contracts::{NodeResourceInventory, NodeResourceSlot};
use chrono::{Duration, Timelike, Utc};
use std::collections::BTreeMap;

fn template(digest_character: char) -> ServiceTemplate {
    let digest = format!("sha256:{}", digest_character.to_string().repeat(64));
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/cloud/fixture@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/fixture".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 250,
            memory_bytes: 64 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: Some(HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5_000,
        }),
    }
}

fn requested_template(uri: &str, expected_digest: Option<String>) -> RequestedServiceTemplate {
    let template = template('a');
    RequestedServiceTemplate {
        artifact: OciArtifactReference {
            uri: uri.into(),
            expected_digest,
        },
        process: template.process,
        secrets: template.secrets,
        resources: template.resources,
        ports: template.ports,
        health: template.health,
    }
}

#[test]
fn headless_service_is_valid_without_network_or_health_policy() {
    let mut headless = template('f');
    headless.ports.clear();
    headless.health = None;

    headless.validate().expect("valid headless Service");
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        WorkloadId::new(),
        1,
        headless,
        Utc::now(),
    )
    .expect("headless revision");
    assert!(revision.request.ports.is_empty());
    assert!(revision.request.health.is_none());
}

#[test]
fn managed_owner_and_effective_placement_are_closed_and_digest_bound() {
    let owner = ManagedOwnerReference::new(
        ManagedOwnerKind::parse("inference.deployment").expect("owner kind"),
        uuid::Uuid::now_v7(),
        7,
        format!("sha256:{}", "a".repeat(64)),
    )
    .expect("managed owner");
    let spec = WorkloadControlSpec::managed_single_replica(owner.clone()).expect("control spec");
    spec.validate().expect("valid control spec");
    assert_eq!(
        spec.managed_owner
            .as_ref()
            .expect("owner")
            .owner_generation(),
        7
    );
    assert_eq!(spec.placement_policy.desired_replicas(), 1);
    assert_eq!(spec.placement_policy.members_per_replica(), 1);
    assert_eq!(
        spec.placement_policy.topology(),
        PlacementTopology::SingleNode
    );
    assert!(spec.placement_policy.digest().starts_with("sha256:"));

    let mut corrupt = spec.placement_policy.document().expect("policy document");
    corrupt["desiredReplicas"] = serde_json::json!(2);
    let corrupt: EffectivePlacementPolicy =
        serde_json::from_value(corrupt).expect("decode corrupt policy");
    assert!(corrupt.validate().is_err());
    assert!(ManagedOwnerKind::parse("InferenceDeployment").is_err());
    assert!(ManagedOwnerReference::new(
        ManagedOwnerKind::parse("inference.deployment").expect("owner kind"),
        uuid::Uuid::nil(),
        1,
        format!("sha256:{}", "b".repeat(64)),
    )
    .is_err());
}

#[test]
fn canonical_replica_identity_survives_generation_advances_and_fences_node_changes() {
    let now = Utc::now();
    let workload = Workload::create(
        WorkloadId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("canonical-replica").expect("name"),
        now,
    );
    let first_revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        template('a'),
        now,
    )
    .expect("first revision");
    let mut replica =
        WorkloadReplica::canonical(&workload, &first_revision).expect("canonical replica");
    let replica_id = replica.id;
    let mut member =
        WorkloadReplicaMember::canonical(&workload, &replica).expect("canonical member");
    let member_id = member.id;
    let first_node = NodeId::new();
    member
        .place(first_node, now + Duration::seconds(1))
        .expect("initial placement");
    assert_eq!(member.placement_generation, 1);
    member
        .place(first_node, now + Duration::seconds(2))
        .expect("idempotent placement");
    assert_eq!(member.placement_generation, 1);
    assert!(member
        .place(NodeId::new(), now + Duration::seconds(3))
        .is_err());

    let second_revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        2,
        template('b'),
        now + Duration::seconds(4),
    )
    .expect("second revision");
    replica
        .advance(&second_revision, now + Duration::seconds(4))
        .expect("advance replica");
    assert_eq!(replica.id, replica_id);
    assert_eq!(member.id, member_id);
    assert_eq!(replica.generation, 2);
    assert_eq!(replica.revision_id, second_revision.id);
}

#[test]
fn deployment_binding_projects_one_provider_identity_for_one_replica_generation() {
    let now = Utc::now();
    let workload = Workload::create(
        WorkloadId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("replica-binding").expect("name"),
        now,
    );
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        template('c'),
        now,
    )
    .expect("revision");
    let replica = WorkloadReplica::canonical(&workload, &revision).expect("replica");
    let mut member = WorkloadReplicaMember::canonical(&workload, &replica).expect("member");
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        now,
    );
    let mut binding = DeploymentReplicaBinding::create(&deployment, &revision, &replica, &member)
        .expect("binding");
    assert_eq!(binding.replica_generation, 1);
    assert_eq!(binding.runtime_unit_id, revision.runtime_unit_id());

    deployment.resolve(now).expect("resolve");
    let node_id = NodeId::new();
    deployment
        .schedule(node_id, now + Duration::seconds(1))
        .expect("schedule");
    member
        .place(node_id, now + Duration::seconds(1))
        .expect("place");
    binding
        .assign(&deployment, &member)
        .expect("bind placement");
    assert_eq!(binding.node_id, Some(node_id));
    assert_eq!(binding.placement_generation, 1);
}

#[test]
fn resource_claim_requires_exact_prepare_bind_and_release_evidence() {
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 789)
        .expect("nanosecond-precision Claim time");
    let binding = placed_replica_binding(now);
    let slots = vec![
        ResourceSlotBinding {
            kind: ResourceKind::Cpu,
            stable_resource_id: "cpu-pool/0-249".into(),
            allocation: ResourceAllocation::Scalar {
                amount: 250,
                unit: ResourceUnit::MilliCpu,
            },
            slot_generation: 3,
            fence_token: uuid::Uuid::new_v4(),
        },
        ResourceSlotBinding {
            kind: ResourceKind::HostPort,
            stable_resource_id: "tcp/18080".into(),
            allocation: ResourceAllocation::Range {
                start: 18_080,
                end_inclusive: 18_080,
                unit: ResourceUnit::Port,
            },
            slot_generation: 8,
            fence_token: uuid::Uuid::new_v4(),
        },
    ];
    let node_id = binding.node_id.expect("placed node");
    let reservation = ResourceClaimReservation {
        id: crate::modules::shared_kernel::domain::ResourceClaimId::new(),
        binding: binding.clone(),
        node_id,
        inventory: inventory_for_bindings(node_id, 11, now, &slots),
        topology_digest: format!("sha256:{}", "2".repeat(64)),
        slots: slots
            .iter()
            .map(|slot| ResourceSlotRequest {
                kind: slot.kind,
                stable_resource_id: slot.stable_resource_id.clone(),
                allocation: slot.allocation.clone(),
            })
            .collect(),
        reserved_at: now,
    };
    let mut claim = ResourceClaim::reserve(&reservation, slots).expect("reserve");
    let prepare_command_id = NodeCommandId::new();
    claim
        .begin_preparation(prepare_command_id, now + Duration::seconds(1))
        .expect("begin prepare");
    let binding_digest = format!("sha256:{}", "3".repeat(64));
    claim
        .record_prepared(
            prepare_command_id,
            binding_digest.clone(),
            now + Duration::seconds(2),
        )
        .expect("prepared");
    claim
        .bind(
            ResourceClaimBindingEvidence {
                runtime_unit_id: binding.runtime_unit_id.clone(),
                runtime_generation: binding.runtime_generation,
                binding_digest,
                slots: claim.slot_evidence(),
                observed_at: now + Duration::seconds(3),
            },
            now + Duration::seconds(3),
        )
        .expect("bind");
    assert_eq!(claim.state, ResourceClaimState::BoundToRuntimeUnit);

    let first_claim_digest = claim.claim_digest.clone();
    let release_command_id = NodeCommandId::new();
    claim
        .begin_release(release_command_id, now + Duration::seconds(4))
        .expect("begin release");
    assert_eq!(claim.claim_generation, 2);
    assert_ne!(claim.claim_digest, first_claim_digest);

    let mut stale_slots = claim.slot_evidence();
    stale_slots[0].slot_generation -= 1;
    assert!(claim
        .record_released(
            ResourceClaimReleaseEvidence::AgentReleased {
                command_id: release_command_id,
                slots: stale_slots,
                evidence_digest: format!("sha256:{}", "4".repeat(64)),
                observed_at: now + Duration::seconds(5),
            },
            now + Duration::seconds(5),
        )
        .is_err());
    claim
        .record_released(
            ResourceClaimReleaseEvidence::AgentReleased {
                command_id: release_command_id,
                slots: claim.slot_evidence(),
                evidence_digest: format!("sha256:{}", "5".repeat(64)),
                observed_at: now + Duration::seconds(5),
            },
            now + Duration::seconds(5),
        )
        .expect("released");
    assert_eq!(claim.state, ResourceClaimState::Released);
    claim.validate().expect("valid released claim");
}

#[test]
fn orphaned_resource_claim_blocks_until_trusted_fencing_evidence() {
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 789)
        .expect("nanosecond-precision Claim time");
    let binding = placed_replica_binding(now);
    let node_id = binding.node_id.expect("placed node");
    let slots = vec![ResourceSlotBinding {
        kind: ResourceKind::Accelerator,
        stable_resource_id: "GPU-00112233".into(),
        allocation: ResourceAllocation::Scalar {
            amount: 1,
            unit: ResourceUnit::Count,
        },
        slot_generation: 9,
        fence_token: uuid::Uuid::new_v4(),
    }];
    let reservation = ResourceClaimReservation {
        id: crate::modules::shared_kernel::domain::ResourceClaimId::new(),
        binding,
        node_id,
        inventory: inventory_for_bindings(node_id, 2, now, &slots),
        topology_digest: format!("sha256:{}", "7".repeat(64)),
        slots: slots
            .iter()
            .map(|slot| ResourceSlotRequest {
                kind: slot.kind,
                stable_resource_id: slot.stable_resource_id.clone(),
                allocation: slot.allocation.clone(),
            })
            .collect(),
        reserved_at: now,
    };
    let mut claim = ResourceClaim::reserve(&reservation, slots).expect("reserve");
    claim
        .orphan(
            "agent disappeared while the claim was reserved".into(),
            now + Duration::seconds(1),
        )
        .expect("orphan");
    assert_eq!(claim.state, ResourceClaimState::Orphaned);
    assert!(claim
        .record_released(
            ResourceClaimReleaseEvidence::ComputeFenced {
                instance_generation: 0,
                slots: claim.slot_evidence(),
                evidence_digest: format!("sha256:{}", "8".repeat(64)),
                observed_at: now + Duration::seconds(2),
            },
            now + Duration::seconds(2),
        )
        .is_err());
    claim
        .record_released(
            ResourceClaimReleaseEvidence::ComputeFenced {
                instance_generation: 3,
                slots: claim.slot_evidence(),
                evidence_digest: format!("sha256:{}", "9".repeat(64)),
                observed_at: now + Duration::seconds(2),
            },
            now + Duration::seconds(2),
        )
        .expect("trusted fence");
    assert_eq!(claim.state, ResourceClaimState::Released);
}

fn inventory_for_bindings(
    node_id: NodeId,
    generation: u64,
    observed_at: chrono::DateTime<Utc>,
    slots: &[ResourceSlotBinding],
) -> NodeResourceInventory {
    NodeResourceInventory::new(
        node_id.as_uuid(),
        uuid::Uuid::now_v7(),
        generation,
        observed_at,
        slots
            .iter()
            .map(|slot| {
                NodeResourceSlot::new(
                    slot.kind,
                    slot.stable_resource_id.clone(),
                    slot.allocation.clone(),
                )
                .expect("inventory slot")
            })
            .collect(),
    )
    .expect("resource inventory")
}

fn placed_replica_binding(now: chrono::DateTime<Utc>) -> DeploymentReplicaBinding {
    let workload = Workload::create(
        WorkloadId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("claim-binding").expect("name"),
        now,
    );
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        template('d'),
        now,
    )
    .expect("revision");
    let replica = WorkloadReplica::canonical(&workload, &revision).expect("replica");
    let mut member = WorkloadReplicaMember::canonical(&workload, &replica).expect("member");
    let node_id = NodeId::new();
    member.place(node_id, now).expect("place member");
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        now,
    );
    deployment.resolve(now).expect("resolve");
    deployment.schedule(node_id, now).expect("schedule");
    DeploymentReplicaBinding::create(&deployment, &revision, &replica, &member).expect("binding")
}

#[test]
fn mutable_oci_reference_resolves_to_one_digest_bound_template() {
    let requested = requested_template("oci://registry.example/cloud/fixture:stable", None);
    let request_digest = requested.request_digest().expect("digest request");
    assert_eq!(
        requested.request_digest().expect("repeat request digest"),
        request_digest
    );

    let digest = format!("sha256:{}", "b".repeat(64));
    let resolved = requested
        .resolve(OciArtifact {
            uri: format!("oci://registry.example/cloud/fixture@{digest}"),
            digest: digest.clone(),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        })
        .expect("resolve tagged request");
    assert_eq!(resolved.artifact.digest, digest);
    assert!(resolved.artifact.uri.contains('@'));

    let wrong_repository = requested_template(
        "oci://registry.example/cloud/fixture:stable",
        Some(format!("sha256:{}", "c".repeat(64))),
    );
    assert!(wrong_repository
        .resolve(OciArtifact {
            uri: format!(
                "oci://registry.example/other/fixture@sha256:{}",
                "c".repeat(64)
            ),
            digest: format!("sha256:{}", "c".repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        })
        .is_err());
}

#[test]
fn oci_request_rejects_implicit_tags_paths_and_digest_conflicts() {
    assert!(
        requested_template("oci://registry.example/cloud/fixture", None)
            .validate_request()
            .is_err()
    );
    assert!(
        requested_template("oci://registry.example/cloud/../fixture:latest", None)
            .validate_request()
            .is_err()
    );
    assert!(requested_template(
        &format!(
            "oci://registry.example/cloud/fixture@sha256:{}",
            "a".repeat(64)
        ),
        Some(format!("sha256:{}", "b".repeat(64))),
    )
    .validate_request()
    .is_err());
}

#[test]
fn revision_requires_a_digest_bound_oci_artifact_and_has_a_stable_digest() {
    let workload_id = WorkloadId::new();
    let created_at = Utc::now();
    let first = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload_id,
        1,
        template('a'),
        created_at,
    )
    .expect("valid revision");
    let replay = WorkloadRevision::create(first.id, workload_id, 1, template('a'), created_at)
        .expect("stable revision");
    assert_eq!(first.template_digest, replay.template_digest);

    let mut mutable = template('b');
    mutable.artifact.uri = "oci://registry.example/cloud/fixture:latest".into();
    assert!(WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload_id,
        2,
        mutable,
        created_at,
    )
    .is_err());

    let mut non_canonical = template('a');
    non_canonical.artifact.digest = format!("sha256:{}", "A".repeat(64));
    non_canonical.artifact.uri = format!(
        "oci://registry.example/cloud/fixture@{}",
        non_canonical.artifact.digest
    );
    assert!(WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload_id,
        2,
        non_canonical,
        created_at,
    )
    .is_err());
}

#[test]
fn external_build_trace_is_validated_and_preserved_by_derived_revisions() {
    let workload_id = WorkloadId::new();
    let secret_id = SecretId::new();
    let created_at = Utc::now();
    let reference = ExternalBuildReference {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        environment_id: EnvironmentId::new(),
        source_revision_id: SourceRevisionId::new(),
        build_run_id: BuildRunId::new(),
    };
    let mut source_template = template('a');
    source_template.secrets = vec![SecretBinding {
        name: "database-url".into(),
        secret_id,
        version: 1,
        target: SecretBindingTarget::Environment {
            variable: "DATABASE_URL".into(),
        },
    }];
    let source = WorkloadRevision::create_from_external_build(
        WorkloadRevisionId::new(),
        workload_id,
        1,
        source_template,
        reference.clone(),
        created_at,
    )
    .expect("external-build revision");
    assert_eq!(source.external_build.as_ref(), Some(&reference));

    let rollback = source
        .rollback_as(
            WorkloadRevisionId::new(),
            2,
            created_at + Duration::seconds(1),
        )
        .expect("rollback revision");
    assert_eq!(rollback.external_build.as_ref(), Some(&reference));

    let restarted = source
        .restart_for_secret_rotation(
            WorkloadRevisionId::new(),
            3,
            secret_id,
            2,
            created_at + Duration::seconds(2),
        )
        .expect("Secret-rotation revision");
    assert_eq!(restarted.external_build.as_ref(), Some(&reference));

    let ordinary = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload_id,
        4,
        template('b'),
        created_at + Duration::seconds(3),
    )
    .expect("ordinary revision");
    assert_eq!(ordinary.external_build, None);

    let mut invalid = reference;
    invalid.build_run_id = BuildRunId::from_uuid(uuid::Uuid::nil());
    assert!(WorkloadRevision::create_from_external_build(
        WorkloadRevisionId::new(),
        workload_id,
        5,
        template('c'),
        invalid,
        created_at + Duration::seconds(4),
    )
    .is_err());
}

#[test]
fn secret_rotation_derives_a_new_resolved_revision_without_mutating_the_source() {
    let workload_id = WorkloadId::new();
    let secret_id = SecretId::new();
    let created_at = Utc::now();
    let mut source_template = template('a');
    source_template.secrets = vec![
        SecretBinding {
            name: "database-environment".into(),
            secret_id,
            version: 2,
            target: SecretBindingTarget::Environment {
                variable: "DATABASE_URL".into(),
            },
        },
        SecretBinding {
            name: "database-file".into(),
            secret_id,
            version: 2,
            target: SecretBindingTarget::File {
                path: "/run/secrets/database-url".into(),
                mode: 0o400,
            },
        },
        SecretBinding {
            name: "unrelated".into(),
            secret_id: SecretId::new(),
            version: 7,
            target: SecretBindingTarget::Environment {
                variable: "UNRELATED".into(),
            },
        },
    ];
    let source = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload_id,
        4,
        source_template,
        created_at,
    )
    .expect("source revision");

    let derived = source
        .restart_for_secret_rotation(
            WorkloadRevisionId::new(),
            6,
            secret_id,
            3,
            created_at + Duration::seconds(1),
        )
        .expect("derived Secret-rotation revision");

    assert_eq!(source.generation, 4);
    assert!(source
        .request
        .secrets
        .iter()
        .filter(|binding| binding.secret_id == secret_id)
        .all(|binding| binding.version == 2));
    assert_eq!(derived.generation, 6);
    assert_eq!(
        derived
            .resolved_template()
            .expect("resolved derived template")
            .artifact,
        source
            .resolved_template()
            .expect("resolved source template")
            .artifact
    );
    assert!(derived
        .request
        .secrets
        .iter()
        .filter(|binding| binding.secret_id == secret_id)
        .all(|binding| binding.version == 3));
    assert_eq!(
        derived
            .request
            .secrets
            .iter()
            .find(|binding| binding.name == "unrelated")
            .expect("unrelated binding")
            .version,
        7
    );
    assert_ne!(derived.request_digest, source.request_digest);
    assert_ne!(derived.template_digest, source.template_digest);
    assert!(source
        .restart_for_secret_rotation(
            WorkloadRevisionId::new(),
            7,
            secret_id,
            2,
            created_at + Duration::seconds(2),
        )
        .is_err());
}

#[test]
fn rollback_clones_the_exact_resolved_template_into_a_new_generation() {
    let workload_id = WorkloadId::new();
    let created_at = Utc::now();
    let source = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload_id,
        2,
        template('b'),
        created_at,
    )
    .expect("source revision");
    let rollback_id = WorkloadRevisionId::new();
    let rollback = source
        .rollback_as(rollback_id, 5, created_at + Duration::seconds(1))
        .expect("rollback revision");

    assert_eq!(rollback.id, rollback_id);
    assert_eq!(rollback.workload_id, workload_id);
    assert_eq!(rollback.generation, 5);
    assert_eq!(rollback.template, source.template);
    assert_eq!(rollback.template_digest, source.template_digest);
    assert_eq!(
        rollback.request.artifact.expected_digest,
        Some(
            source
                .resolved_template()
                .expect("source template")
                .artifact
                .digest
                .clone()
        )
    );
    assert_ne!(rollback.id, source.id);

    assert!(source
        .rollback_as(source.id, 6, created_at + Duration::seconds(2))
        .is_err());
    assert!(source
        .rollback_as(
            WorkloadRevisionId::new(),
            source.generation,
            created_at + Duration::seconds(2),
        )
        .is_err());
    assert!(source
        .rollback_as(
            WorkloadRevisionId::new(),
            6,
            created_at - Duration::seconds(1)
        )
        .is_err());

    let unresolved = WorkloadRevision::request(
        WorkloadRevisionId::new(),
        workload_id,
        3,
        requested_template("oci://registry.example/cloud/fixture:next", None),
        created_at,
    )
    .expect("unresolved source");
    assert!(unresolved
        .rollback_as(
            WorkloadRevisionId::new(),
            7,
            created_at + Duration::seconds(2),
        )
        .is_err());
}

#[test]
fn deployment_lifecycle_is_monotonic_and_activation_selects_the_revision() {
    let now = Utc::now();
    let now = now
        .with_nanosecond(now.nanosecond() / 1_000 * 1_000 + 789)
        .expect("sub-microsecond workload timestamp");
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let mut workload = Workload::create(
        workload_id,
        OrganizationId::new(),
        crate::modules::shared_kernel::domain::ProjectId::new(),
        crate::modules::shared_kernel::domain::EnvironmentId::new(),
        ResourceName::parse("fixture").expect("workload name"),
        now,
    );
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload_id,
        revision_id,
        OperationId::new(),
        now,
    );
    deployment.resolve(now).expect("resolve");
    deployment
        .schedule(NodeId::new(), now + Duration::seconds(1))
        .expect("schedule");
    deployment
        .dispatch(NodeCommandId::new(), now + Duration::seconds(2))
        .expect("dispatch");
    deployment
        .verify(now + Duration::seconds(3))
        .expect("verify");
    assert!(deployment
        .request_cancellation(now + Duration::seconds(4))
        .is_err());
    deployment
        .activate(false, now + Duration::seconds(4))
        .expect("activate");
    workload
        .activate(revision_id, now + Duration::seconds(4))
        .expect("select active revision");
    assert_eq!(deployment.status, DeploymentStatus::Active);
    assert_eq!(workload.active_revision_id, Some(revision_id));
    assert_eq!(deployment.requested_at.nanosecond() % 1_000, 0);
    assert_eq!(deployment.updated_at.nanosecond() % 1_000, 0);
    assert_eq!(workload.created_at.nanosecond() % 1_000, 0);
    assert_eq!(workload.updated_at.nanosecond() % 1_000, 0);
    assert!(deployment
        .fail("late failure".into(), now + Duration::seconds(5))
        .is_err());
}

#[test]
fn activated_update_retires_the_previous_runtime_before_becoming_terminal() {
    let now = Utc::now();
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        OrganizationId::new(),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        OperationId::new(),
        now,
    );
    deployment.resolve(now).expect("resolve");
    deployment.schedule(NodeId::new(), now).expect("schedule");
    deployment
        .dispatch(NodeCommandId::new(), now)
        .expect("dispatch");
    deployment.verify(now).expect("verify");
    deployment
        .activate(true, now + Duration::seconds(1))
        .expect("activate update");
    assert_eq!(deployment.status, DeploymentStatus::Retiring);
    assert!(!deployment.status.is_terminal());

    let retirement_command_id = NodeCommandId::new();
    deployment
        .dispatch_retirement(retirement_command_id, now + Duration::seconds(2))
        .expect("dispatch retirement");
    assert_eq!(
        deployment.retirement_command_id,
        Some(retirement_command_id)
    );
    deployment
        .complete_retirement(now + Duration::seconds(3))
        .expect("complete retirement");
    assert_eq!(deployment.status, DeploymentStatus::Active);
    assert!(deployment.status.is_terminal());
    assert!(deployment
        .activate(false, now + Duration::seconds(4))
        .is_err());
}

#[test]
fn workload_stop_is_two_phase_idempotent_and_blocks_late_activation() {
    let now = Utc::now();
    let revision_id = WorkloadRevisionId::new();
    let mut workload = Workload::create(
        WorkloadId::new(),
        OrganizationId::new(),
        crate::modules::shared_kernel::domain::ProjectId::new(),
        crate::modules::shared_kernel::domain::EnvironmentId::new(),
        ResourceName::parse("stop fixture").expect("workload name"),
        now,
    );
    workload
        .activate(revision_id, now + Duration::seconds(1))
        .expect("activate workload");
    workload
        .request_stop(now + Duration::seconds(2))
        .expect("request stop");
    let requested_version = workload.aggregate_version;
    workload
        .request_stop(now + Duration::seconds(3))
        .expect("replay stop request");
    assert_eq!(workload.aggregate_version, requested_version);
    assert_eq!(workload.active_revision_id, Some(revision_id));
    assert!(workload
        .activate(WorkloadRevisionId::new(), now + Duration::seconds(3))
        .is_err());
    workload
        .complete_stop(now + Duration::seconds(4))
        .expect("complete stop");
    let completed_version = workload.aggregate_version;
    workload
        .complete_stop(now + Duration::seconds(5))
        .expect("replay stop completion");
    assert_eq!(workload.aggregate_version, completed_version);
    assert_eq!(workload.active_revision_id, None);
    assert_eq!(workload.desired_state, WorkloadDesiredState::Stopped);
}

#[test]
fn deployment_rejects_identity_changes_and_failed_transitions_are_atomic() {
    let now = Utc::now();
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        OrganizationId::new(),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        OperationId::new(),
        now,
    );
    deployment.resolve(now).expect("resolve");
    let node_id = NodeId::new();
    deployment
        .schedule(node_id, now + Duration::seconds(2))
        .expect("schedule");
    let scheduled = deployment.clone();

    assert!(deployment
        .schedule(NodeId::new(), now + Duration::seconds(3))
        .is_err());
    assert_eq!(deployment, scheduled);

    assert!(deployment
        .dispatch(NodeCommandId::new(), now + Duration::seconds(1))
        .is_err());
    assert_eq!(deployment, scheduled);

    let command_id = NodeCommandId::new();
    deployment
        .dispatch(command_id, now + Duration::seconds(3))
        .expect("dispatch");
    let dispatched = deployment.clone();
    assert!(deployment
        .dispatch(NodeCommandId::new(), now + Duration::seconds(4))
        .is_err());
    assert_eq!(deployment, dispatched);
}

#[test]
fn cancellation_is_terminal_and_idempotent() {
    let now = Utc::now();
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        OrganizationId::new(),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        OperationId::new(),
        now,
    );
    deployment.cancel(now).expect("cancel queued deployment");
    let cancelled = deployment.clone();
    deployment.cancel(now).expect("repeat cancellation");
    assert_eq!(deployment, cancelled);
    assert!(deployment.fail("late failure".into(), now).is_err());
}

#[test]
fn dispatched_cancellation_tracks_cleanup_before_becoming_terminal() {
    let now = Utc::now();
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        OrganizationId::new(),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        OperationId::new(),
        now,
    );
    deployment.resolve(now).expect("resolve");
    deployment.schedule(NodeId::new(), now).expect("schedule");
    deployment
        .dispatch(NodeCommandId::new(), now)
        .expect("dispatch");
    deployment
        .request_cancellation(now + Duration::seconds(1))
        .expect("request cancellation");
    assert_eq!(deployment.status, DeploymentStatus::Cancelling);
    assert!(deployment.cancelled_at.is_none());

    let cleanup_command_id = NodeCommandId::new();
    deployment
        .begin_cleanup(cleanup_command_id, now + Duration::seconds(2))
        .expect("begin cleanup");
    assert_eq!(deployment.status, DeploymentStatus::CleanupPending);
    assert_eq!(deployment.cleanup_command_id, Some(cleanup_command_id));

    let retry_command_id = NodeCommandId::new();
    deployment
        .retry_cleanup(retry_command_id, now + Duration::seconds(3))
        .expect("retry cleanup");
    assert_eq!(deployment.cleanup_command_id, Some(retry_command_id));

    deployment
        .cancel(now + Duration::seconds(4))
        .expect("complete cancellation");
    assert_eq!(deployment.status, DeploymentStatus::Cancelled);
    assert_eq!(
        deployment.cancelled_at,
        Some(canonical_timestamp(now + Duration::seconds(4)))
    );
}

#[test]
fn cleanup_failure_is_an_operator_visible_orphan() {
    let now = Utc::now();
    let mut deployment = Deployment::create(
        DeploymentId::new(),
        OrganizationId::new(),
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        OperationId::new(),
        now,
    );
    deployment.resolve(now).expect("resolve");
    deployment.schedule(NodeId::new(), now).expect("schedule");
    deployment
        .dispatch(NodeCommandId::new(), now)
        .expect("dispatch");
    deployment
        .request_cancellation(now)
        .expect("request cancellation");
    deployment
        .begin_cleanup(NodeCommandId::new(), now)
        .expect("begin cleanup");
    deployment
        .fail(
            "cleanup deadline expired".into(),
            now + Duration::minutes(1),
        )
        .expect("record orphan");
    assert_eq!(deployment.status, DeploymentStatus::Orphaned);
    assert_eq!(
        deployment.failure.as_deref(),
        Some("cleanup deadline expired")
    );
}
