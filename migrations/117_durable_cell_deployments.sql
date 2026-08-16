create table durable_cell_deployments (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    application_id uuid not null,
    application_revision_id uuid not null,
    application_revision_number bigint not null
        check (application_revision_number > 0),
    application_definition_digest text not null
        check (application_definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    storage_namespace_id uuid not null,
    credential_binding_generation bigint not null
        check (credential_binding_generation > 0),
    credential_binding_digest text not null
        check (credential_binding_digest ~ '^sha256:[0-9a-f]{64}$'),
    storage_provider_profile_digest text not null
        check (storage_provider_profile_digest ~ '^sha256:[0-9a-f]{64}$'),
    retention_policy_digest text not null
        check (retention_policy_digest ~ '^sha256:[0-9a-f]{64}$'),
    workload_id uuid not null,
    workload_revision_id uuid not null,
    workload_generation bigint not null check (workload_generation > 0),
    service_profile_digest text not null
        check (service_profile_digest ~ '^sha256:[0-9a-f]{64}$'),
    service_template_digest text not null
        check (service_template_digest ~ '^sha256:[0-9a-f]{64}$'),
    provider_artifact_digest text not null
        check (provider_artifact_digest ~ '^sha256:[0-9a-f]{64}$'),
    deployment_id uuid not null,
    operation_id uuid not null,
    placement_policy_digest text not null
        check (placement_policy_digest ~ '^sha256:[0-9a-f]{64}$'),
    requested_by uuid not null references identity_principals(id),
    request_id uuid not null,
    requested_at timestamptz not null,
    primary key (organization_id, application_id, application_revision_id),
    unique (workload_revision_id),
    unique (deployment_id),
    unique (operation_id),
    foreign key (
        organization_id,
        project_id,
        environment_id,
        application_id,
        application_revision_id
    ) references durable_cell_application_revisions (
        organization_id,
        project_id,
        environment_id,
        application_id,
        id
    )
);

create index durable_cell_deployments_environment_idx
    on durable_cell_deployments (
        organization_id,
        project_id,
        environment_id,
        application_id,
        application_revision_number desc
    );

create function reject_durable_cell_deployment_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Durable Cell deployment correlations are immutable';
end
$$;

create trigger durable_cell_deployments_immutable
before update or delete on durable_cell_deployments
for each row execute function reject_durable_cell_deployment_mutation();

comment on table durable_cell_deployments is
    'Immutable process-recovery correlation only; Workloads owns Deployment, Operations owns execution, Fleet owns receipts, and S0 owns namespace behavior';

comment on column durable_cell_deployments.deployment_id is
    'Deterministic existing Workloads identity; intentionally not a second deployment authority';

comment on column durable_cell_deployments.storage_namespace_id is
    'Exact S0 binding identity; intentionally not a namespace lifecycle record';
