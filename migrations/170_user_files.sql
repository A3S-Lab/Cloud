create table user_file_organization_quotas (
    organization_id uuid primary key references organizations(id),
    limit_bytes bigint not null check (limit_bytes between 1 and 9007199254740991),
    allocated_bytes bigint not null check (
        allocated_bytes >= 0 and allocated_bytes <= limit_bytes
    ),
    revision bigint not null check (revision between 0 and 9007199254740991),
    updated_at timestamptz,
    check ((revision = 0) = (updated_at is null))
);

comment on table user_file_organization_quotas is
    'Atomic K0 Files allocation ledger; it is updated only in the same transaction as the canonical UserFile lifecycle and does not define a second quota lifecycle';

create table user_files (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    upload_id uuid not null,
    contract_schema text not null check (contract_schema = 'cloud.user-file.v1'),
    canonical_acl text not null check (octet_length(canonical_acl) between 1 and 65536),
    contract_digest text not null check (contract_digest ~ '^sha256:[0-9a-f]{64}$'),
    size_bytes bigint not null check (size_bytes between 1 and 536870912),
    upload_expires_at timestamptz not null,
    retention_until timestamptz not null,
    state text not null check (
        state in (
            'awaiting_upload', 'awaiting_scan', 'admitted',
            'rejected', 'expired', 'tombstoned'
        )
    ),
    scan_evidence_digest text check (scan_evidence_digest ~ '^sha256:[0-9a-f]{64}$'),
    rejection_reason_code text check (
        octet_length(rejection_reason_code) between 1 and 64
        and rejection_reason_code ~ '^[a-z0-9._-]+$'
    ),
    tombstoned_from text check (
        tombstoned_from in (
            'awaiting_upload', 'awaiting_scan', 'admitted', 'rejected', 'expired'
        )
    ),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_by uuid not null,
    created_at timestamptz not null,
    uploaded_at timestamptz,
    scanned_at timestamptz,
    expired_at timestamptz,
    tombstoned_at timestamptz,
    updated_at timestamptz not null,
    cleanup_due_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, upload_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id)
        references user_file_organization_quotas (organization_id),
    check (upload_expires_at > created_at),
    check (retention_until > upload_expires_at),
    check (updated_at >= created_at),
    check (uploaded_at is null or (uploaded_at >= created_at and uploaded_at < upload_expires_at)),
    check (scanned_at is null or (uploaded_at is not null and scanned_at >= uploaded_at and scanned_at < retention_until)),
    check (expired_at is null or expired_at >= upload_expires_at),
    check (tombstoned_at is null or tombstoned_at >= created_at),
    check (
        aggregate_version = case
            when state = 'awaiting_upload' then 1
            when state = 'awaiting_scan' then 2
            when state in ('admitted', 'rejected') then 3
            when state = 'expired' then 2
            when state = 'tombstoned' and tombstoned_from = 'awaiting_upload' then 2
            when state = 'tombstoned' and tombstoned_from = 'awaiting_scan' then 3
            when state = 'tombstoned' and tombstoned_from in ('admitted', 'rejected') then 4
            when state = 'tombstoned' and tombstoned_from = 'expired' then 3
            else 0
        end
    ),
    check (
        (state = 'awaiting_upload' and uploaded_at is null and scanned_at is null and expired_at is null and tombstoned_at is null)
        or (state = 'awaiting_scan' and uploaded_at is not null and scanned_at is null and expired_at is null and tombstoned_at is null)
        or (state in ('admitted', 'rejected') and uploaded_at is not null and scanned_at is not null and expired_at is null and tombstoned_at is null)
        or (state = 'expired' and uploaded_at is null and scanned_at is null and expired_at is not null and tombstoned_at is null)
        or (state = 'tombstoned' and tombstoned_at is not null)
    ),
    check ((state = 'tombstoned') = (tombstoned_from is not null)),
    check (
        (scan_evidence_digest is not null) = (
            state in ('admitted', 'rejected')
            or (state = 'tombstoned' and tombstoned_from in ('admitted', 'rejected'))
        )
    ),
    check (
        (rejection_reason_code is not null) = (
            state = 'rejected'
            or (state = 'tombstoned' and tombstoned_from = 'rejected')
        )
    ),
    check (
        (expired_at is not null) = (
            state = 'expired'
            or (state = 'tombstoned' and tombstoned_from = 'expired')
        )
    ),
    check (
        cleanup_due_at is not distinct from case
            when state in ('awaiting_scan', 'admitted') then retention_until
            when state = 'rejected' then updated_at
            when state = 'tombstoned' and uploaded_at is not null then updated_at
            else null
        end
    )
);

create index user_files_project_time_idx
    on user_files (organization_id, project_id, created_at, id);

create index user_files_upload_expiration_idx
    on user_files (upload_expires_at, organization_id, project_id, id)
    where state = 'awaiting_upload';

create index user_files_cleanup_due_idx
    on user_files (cleanup_due_at, organization_id, project_id, id)
    where cleanup_due_at is not null;

comment on table user_files is
    'Canonical K0 UserFile metadata and lifecycle; canonical_acl is the only product contract, size and deadlines are verified projections, bytes remain in the shared immutable-object authority, and cleanup intent is emitted through the shared Outbox rather than a Files-local queue';
