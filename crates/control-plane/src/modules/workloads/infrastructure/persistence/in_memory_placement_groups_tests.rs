use super::InMemoryWorkloadRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, ResourceName, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, OciArtifact, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    Workload, WorkloadControlSpec, WorkloadPlacementGroup, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::{
    IWorkloadPlacementGroupRepository, IWorkloadReplicaDeploymentRepository, IWorkloadRepository,
};
use chrono::Utc;
use std::collections::BTreeMap;

struct PlacementGroupFixture {
    workload: Workload,
    spec: WorkloadControlSpec,
    revision: WorkloadRevision,
    templates: Vec<ServiceTemplate>,
}

#[tokio::test]
async fn placement_group_materialization_is_atomic_replay_safe_and_immutable() {
    let repository = InMemoryWorkloadRepository::new();
    let PlacementGroupFixture {
        workload,
        spec,
        revision,
        templates,
    } = fixture().expect("fixture");
    let organization_id = workload.organization_id;
    let policy = spec.placement_policy.clone();
    let replica = repository
        .seed_placement_group_foundation(workload.clone(), spec, revision.clone())
        .await
        .expect("seed placement-group foundation");
    let write = WorkloadPlacementGroup::plan(
        &workload,
        &policy,
        &revision,
        &replica,
        templates,
        Utc::now().max(replica.updated_at),
    )
    .expect("group write");

    let created = repository
        .materialize_placement_group(write.clone())
        .await
        .expect("materialize group");
    let replayed = repository
        .materialize_placement_group(write.clone())
        .await
        .expect("replay group");
    assert!(!created.replayed);
    assert!(replayed.replayed);
    assert_eq!(created.group, replayed.group);
    assert_eq!(created.replica_members, replayed.replica_members);
    assert_eq!(
        repository
            .find_placement_group_for_replica_generation(
                organization_id,
                replica.id,
                replica.generation,
            )
            .await
            .expect("group by replica generation"),
        created.group
    );
    assert_eq!(
        repository
            .list_workload_replica_members(organization_id, replica.id)
            .await
            .expect("members")
            .len(),
        3
    );

    let mut changed_templates = changed_templates(&revision);
    changed_templates[1] = template('d');
    let changed = WorkloadPlacementGroup::plan(
        &workload,
        &policy,
        &revision,
        &replica,
        changed_templates,
        write.group.created_at,
    )
    .expect("changed valid group plan");
    assert_eq!(
        repository.materialize_placement_group(changed).await,
        Err(RepositoryError::IdempotencyConflict)
    );
}

#[tokio::test]
async fn legacy_replica_deployment_materializer_skips_multi_node_plans() {
    let repository = InMemoryWorkloadRepository::new();
    let PlacementGroupFixture {
        workload,
        spec,
        revision,
        ..
    } = fixture().expect("fixture");
    repository
        .seed_placement_group_foundation(workload, spec, revision)
        .await
        .expect("seed placement-group foundation");
    assert!(repository
        .pending_replica_deployments(10)
        .await
        .expect("deployment candidates")
        .is_empty());
}

fn fixture() -> Result<PlacementGroupFixture, Box<dyn std::error::Error>> {
    let now = Utc::now();
    let workload = Workload::create(
        WorkloadId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("placement-group repository")?,
        now,
    );
    let leader = template('a');
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        leader.clone(),
        now,
    )?;
    Ok(PlacementGroupFixture {
        workload,
        spec: WorkloadControlSpec::unmanaged_placement_group(1, 1, 3)?,
        revision,
        templates: vec![leader, template('b'), template('c')],
    })
}

fn changed_templates(revision: &WorkloadRevision) -> Vec<ServiceTemplate> {
    vec![
        revision.resolved_template().expect("leader").clone(),
        template('b'),
        template('c'),
    ]
}

fn template(digest_character: char) -> ServiceTemplate {
    let digest = format!("sha256:{}", digest_character.to_string().repeat(64));
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/cloud/group@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/service".into()],
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
