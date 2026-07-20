alter table secrets
    add constraint secrets_organization_id_id_unique
    unique (organization_id, id);

create table secret_rotation_restarts (
    secret_event_id uuid not null references outbox_events(event_id),
    organization_id uuid not null,
    secret_id uuid not null,
    secret_version bigint not null check (secret_version > 0),
    workload_id uuid not null,
    source_revision_id uuid not null,
    target_revision_id uuid not null,
    deployment_id uuid not null unique references deployments(id),
    operation_id uuid not null unique,
    created_at timestamptz not null,
    primary key (secret_event_id, workload_id),
    foreign key (secret_id, secret_version)
        references secret_versions (secret_id, version),
    foreign key (organization_id, secret_id)
        references secrets (organization_id, id),
    foreign key (organization_id, workload_id)
        references workloads (organization_id, id),
    foreign key (workload_id, source_revision_id)
        references workload_revisions (workload_id, id),
    foreign key (workload_id, target_revision_id)
        references workload_revisions (workload_id, id),
    foreign key (organization_id, operation_id)
        references operation_requests (organization_id, operation_id)
);

create index secret_rotation_restarts_secret_idx
    on secret_rotation_restarts (
        organization_id,
        secret_id,
        secret_version,
        created_at,
        workload_id
    );

create table secret_rotation_reconciliations (
    secret_event_id uuid primary key references outbox_events(event_id),
    organization_id uuid not null references organizations(id),
    secret_id uuid not null,
    secret_version bigint not null check (secret_version > 0),
    outcome text not null check (
        outcome in ('scheduled', 'no_targets', 'superseded', 'unavailable')
    ),
    restart_count integer not null check (restart_count >= 0),
    reconciled_at timestamptz not null,
    foreign key (secret_id, secret_version)
        references secret_versions (secret_id, version),
    foreign key (organization_id, secret_id)
        references secrets (organization_id, id),
    check (
        outcome = 'scheduled' and restart_count > 0
        or outcome <> 'scheduled'
    )
);

create index outbox_secret_rotation_restart_idx
    on outbox_events (occurred_at, event_id)
    where event_key = 'secret.version.created';
