const MIGRATION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../migrations/180_deployment_runtime_execution_bindings.sql"
));

#[test]
fn migration_180_adds_one_immutable_pre_scheduling_runtime_authority() {
    let lower = MIGRATION.to_ascii_lowercase();
    for required in [
        "create table deployment_runtime_execution_bindings",
        "a3s.cloud.deployment-runtime-execution-binding.v1",
        "deployment_id uuid primary key references deployments",
        "runtime_class is null or runtime_class = 'service'",
        "admitted_at timestamptz not null",
        "authorized_at is null or authorized_at <= admitted_at",
        "new.admitted_at < owner.updated_at",
        "including explicit no-policy outcomes",
        "identity_attachment_digest",
        "semantics_profile_digest",
        "binding_digest",
        "owner.status <> 'resolving'",
        "for update of deployment",
        "for update of control",
        "foreign key (organization_id, project_id, environment_id, workload_id)",
        "bindings are immutable",
        "legacy deployments are deliberately not backfilled",
    ] {
        assert!(
            lower.contains(&required.to_ascii_lowercase()),
            "migration 180 is missing {required}"
        );
    }
    for forbidden in [
        "update deployments",
        "update workload_revisions",
        "insert into workload_identity_policy",
        "policy_id",
        "credential",
        "private_key",
        "create queue",
        "provider_config",
        "json_config",
        "yaml_config",
    ] {
        assert!(
            !lower.contains(forbidden),
            "migration 180 duplicated another owner through {forbidden}"
        );
    }

    let deployment_lock = lower
        .find("for update of deployment")
        .expect("Deployment lock");
    let control_lock = lower
        .find("for update of control")
        .expect("Workload control lock");
    assert!(
        deployment_lock < control_lock,
        "migration 180 must lock Deployment before Workload control"
    );
}
