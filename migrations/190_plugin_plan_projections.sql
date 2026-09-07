-- U0.3: immutable Cloud review projection for one A3S Use operation plan.
-- Stores digests and bounded metadata only; never package bytes, locks, Secret
-- values, or a mutable plan document.

create table plugin_plan_projections (
    organization_id uuid not null check (
        organization_id <> '00000000-0000-0000-0000-000000000000'
    ),
    id uuid primary key check (
        id <> '00000000-0000-0000-0000-000000000000'
    ),
    assignment_id uuid not null check (
        assignment_id <> '00000000-0000-0000-0000-000000000000'
    ),
    operation_id uuid not null check (
        operation_id <> '00000000-0000-0000-0000-000000000000'
    ),
    assignment_generation bigint not null check (assignment_generation > 0),
    use_operation_id text not null check (
        char_length(use_operation_id) between 1 and 256
    ),
    plan_schema text not null check (
        plan_schema = 'a3s.use.plugin-operation-plan.v4'
    ),
    plan_digest text not null check (
        plan_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    expires_at timestamptz not null,
    action text not null check (
        action in ('install', 'uninstall', 'upgrade', 'enable', 'disable')
    ),
    root_package_id text not null check (
        char_length(root_package_id) between 3 and 127
        and root_package_id ~ '^[a-z][a-z0-9-]*/[a-z][a-z0-9-]*$'
    ),
    root_package_digest text check (
        root_package_digest is null
        or root_package_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    root_manifest_digest text check (
        root_manifest_digest is null
        or root_manifest_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    authority_decision text not null check (
        authority_decision in ('allow', 'ask', 'deny')
    ),
    authority_policy_digest text not null check (
        authority_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    impact_digest text not null check (
        impact_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    permission_evidence_digest text not null check (
        permission_evidence_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    provider_evidence_digest text not null check (
        provider_evidence_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    confirmation_digest text check (
        confirmation_digest is null
        or confirmation_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    terminal_reason text check (
        terminal_reason is null
        or char_length(terminal_reason) between 1 and 512
    ),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    unique (organization_id, id),
    unique (organization_id, operation_id, plan_digest),
    foreign key (organization_id, assignment_id)
        references plugin_assignments (organization_id, id),
    check (updated_at >= created_at)
);

create index plugin_plan_projections_assignment_idx
    on plugin_plan_projections (
        organization_id,
        assignment_id,
        created_at,
        id
    );

create index plugin_plan_projections_operation_idx
    on plugin_plan_projections (
        organization_id,
        operation_id,
        plan_digest
    );
