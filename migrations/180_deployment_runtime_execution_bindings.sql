create table deployment_runtime_execution_bindings (
    deployment_id uuid primary key references deployments (id),
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    workload_revision_id uuid not null,
    node_pool_id uuid,
    binding_schema text not null check (
        binding_schema = 'a3s.cloud.deployment-runtime-execution-binding.v1'
    ),
    runtime_class text check (
        runtime_class is null or runtime_class = 'service'
    ),
    isolation_level text check (
        isolation_level is null
        or isolation_level in ('process', 'container', 'sandbox', 'confidential')
    ),
    semantics_profile_digest text check (
        semantics_profile_digest is null
        or semantics_profile_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    identity_attachment_digest text check (
        identity_attachment_digest is null
        or identity_attachment_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    authorized_at timestamptz,
    admitted_at timestamptz not null,
    binding_digest text not null check (
        binding_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    check (
        (
            runtime_class is null
            and isolation_level is null
            and semantics_profile_digest is null
            and identity_attachment_digest is null
            and authorized_at is null
        )
        or (
            node_pool_id is not null
            and runtime_class = 'service'
            and isolation_level is not null
            and semantics_profile_digest is not null
            and identity_attachment_digest is not null
            and authorized_at is not null
        )
    ),
    check (authorized_at is null or authorized_at <= admitted_at),
    foreign key (organization_id, project_id, environment_id, workload_id)
        references workloads (organization_id, project_id, environment_id, id),
    foreign key (workload_id, workload_revision_id)
        references workload_revisions (workload_id, id),
    foreign key (organization_id, node_pool_id)
        references node_pools (organization_id, id)
);

create function validate_deployment_runtime_execution_binding()
returns trigger
language plpgsql
as $$
declare
    owner record;
    control_node_pool_id uuid;
begin
    if tg_op <> 'INSERT' then
        raise exception 'Deployment Runtime execution bindings are immutable'
            using errcode = '23514';
    end if;

    select deployment.organization_id,
           deployment.workload_id,
           deployment.revision_id,
           deployment.node_id,
           deployment.command_id,
           deployment.cleanup_command_id,
           deployment.retirement_command_id,
           deployment.status,
           deployment.updated_at,
           deployment.activated_at,
           deployment.cancellation_requested_at,
           deployment.cancelled_at,
           workload.project_id,
           workload.environment_id
      into owner
      from deployments deployment
      join workloads workload
        on workload.organization_id = deployment.organization_id
       and workload.id = deployment.workload_id
     where deployment.id = new.deployment_id
       for update of deployment;

    if not found then
        raise exception 'Deployment Runtime execution binding has no exact owner lineage'
            using errcode = '23503';
    end if;

    -- Keep the same canonical lock order as Deployment scheduling and the
    -- Workloads repository: Deployment -> Workload Control. Separate lock
    -- statements make this ordering independent of the PostgreSQL join plan.
    select control.node_pool_id
      into control_node_pool_id
      from workload_controls control
     where control.organization_id = owner.organization_id
       and control.workload_id = owner.workload_id
       for update of control;

    if not found then
        raise exception 'Deployment Runtime execution binding has no Workload control'
            using errcode = '23503';
    end if;

    if owner.organization_id is distinct from new.organization_id
       or owner.project_id is distinct from new.project_id
       or owner.environment_id is distinct from new.environment_id
       or owner.workload_id is distinct from new.workload_id
       or owner.revision_id is distinct from new.workload_revision_id
       or control_node_pool_id is distinct from new.node_pool_id then
        raise exception 'Deployment Runtime execution binding changed owner lineage or NodePool'
            using errcode = '23514';
    end if;
    if owner.status <> 'resolving'
       or new.admitted_at < owner.updated_at
       or owner.node_id is not null
       or owner.command_id is not null
       or owner.cleanup_command_id is not null
       or owner.retirement_command_id is not null
       or owner.activated_at is not null
       or owner.cancellation_requested_at is not null
       or owner.cancelled_at is not null then
        raise exception 'Deployment Runtime execution binding requires an unresolved Deployment'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger deployment_runtime_execution_bindings_immutable_admission
before insert or update or delete on deployment_runtime_execution_bindings
for each row execute function validate_deployment_runtime_execution_binding();

comment on table deployment_runtime_execution_bindings is
    'Workloads-owned immutable provider-neutral Runtime admission, including explicit no-policy outcomes, committed once before scheduling; legacy Deployments are deliberately not backfilled';
