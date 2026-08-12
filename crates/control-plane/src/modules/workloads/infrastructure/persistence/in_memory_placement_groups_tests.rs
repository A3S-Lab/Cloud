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
use crate::modules::workloads::{
    PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME, PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
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

    let candidates = repository
        .pending_replica_deployments(10)
        .await
        .expect("placement-group Deployment candidates");
    assert_eq!(candidates.len(), 1);
    let candidate = candidates[0];
    let requested_at = Utc::now().max(created.group.updated_at);
    let (left, right) = tokio::join!(
        repository.materialize_replica_deployment(candidate, requested_at),
        repository.materialize_replica_deployment(candidate, requested_at),
    );
    let left = left
        .expect("left group Deployment materialization")
        .expect("left group Deployment result");
    let right = right
        .expect("right group Deployment materialization")
        .expect("right group Deployment result");
    assert_ne!(left.created, right.created);
    assert_eq!(left.deployment, right.deployment);
    assert_eq!(left.member_bindings, right.member_bindings);
    assert_eq!(left.placement_group_binding, right.placement_group_binding);
    let materialization = if left.created { &left } else { &right };
    assert_eq!(
        materialization.operation.workflow.name(),
        PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME
    );
    assert_eq!(
        materialization.operation.workflow.version(),
        PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION
    );
    assert_eq!(materialization.member_bindings.len(), 3);
    assert!(materialization
        .member_bindings
        .iter()
        .all(|binding| binding.node_id.is_none() && binding.placement_generation == 0));
    let group_binding = materialization
        .placement_group_binding
        .as_ref()
        .expect("group Deployment binding");
    assert_eq!(group_binding.group_id, created.group.id);
    assert_eq!(group_binding.group_plan_digest, created.group.plan_digest);
    assert_eq!(group_binding.member_count, 3);
    assert_eq!(
        repository
            .list_deployment_replica_member_bindings(
                organization_id,
                materialization.deployment.id,
            )
            .await
            .expect("stored group Deployment member bindings"),
        materialization.member_bindings
    );
    assert_eq!(
        repository
            .find_deployment_placement_group_binding(
                organization_id,
                materialization.deployment.id,
            )
            .await
            .expect("stored group Deployment binding"),
        *group_binding
    );
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "workload.deployment.requested")
            .count(),
        1
    );
    assert!(repository
        .pending_replica_deployments(10)
        .await
        .expect("replayed placement-group Deployment candidates")
        .is_empty());

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
