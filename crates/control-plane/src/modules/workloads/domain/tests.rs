use super::entities::*;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, NodeCommandId, NodeId, OperationId, OrganizationId,
    ResourceName, SecretId, WorkloadId, WorkloadRevisionId,
};
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
        health: HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5_000,
        },
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
    deployment
        .activate(now + Duration::seconds(4))
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
