create table organizations (
    id uuid primary key,
    name text not null,
    name_key text not null unique,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null
);

create table projects (
    organization_id uuid not null references organizations(id),
    id uuid not null,
    name text not null,
    name_key text not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, name_key)
);

create table environments (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    name text not null,
    name_key text not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    primary key (organization_id, project_id, id),
    unique (organization_id, project_id, name_key),
    foreign key (organization_id, project_id)
        references projects (organization_id, id)
);

create table idempotency_records (
    scope_key text not null,
    idempotency_key text not null,
    request_digest text not null,
    response jsonb not null,
    created_at timestamptz not null,
    primary key (scope_key, idempotency_key)
);

create table outbox_events (
    event_id uuid primary key,
    event_key text not null,
    schema_version integer not null check (schema_version > 0),
    organization_id uuid not null,
    aggregate_id uuid not null,
    aggregate_version bigint not null check (aggregate_version > 0),
    occurred_at timestamptz not null,
    correlation_id uuid not null,
    causation_id uuid,
    payload jsonb not null,
    published_at timestamptz,
    delivery_attempts integer not null default 0 check (delivery_attempts >= 0),
    last_error text
);

create index outbox_events_pending_idx
    on outbox_events (occurred_at, event_id)
    where published_at is null;

create table audit_records (
    audit_id uuid primary key,
    organization_id uuid not null,
    actor_id uuid,
    action text not null,
    aggregate_id uuid not null,
    occurred_at timestamptz not null,
    request_id uuid not null,
    details jsonb not null
);

create index audit_records_organization_time_idx
    on audit_records (organization_id, occurred_at desc, audit_id desc);
