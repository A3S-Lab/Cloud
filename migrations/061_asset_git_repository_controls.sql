create table asset_git_repository_controls (
    organization_id uuid not null,
    asset_id uuid not null,
    quota_bytes bigint not null check (quota_bytes > 0),
    observed_bytes bigint not null check (observed_bytes >= 0),
    write_lease_id uuid,
    write_lease_operation text check (
        write_lease_operation in ('receive_pack', 'backup', 'restore')
    ),
    write_lease_actor_id uuid,
    write_lease_request_id uuid,
    write_leased_until timestamptz,
    write_lease_recovering boolean not null default false,
    write_cleanup_lease_id uuid,
    latest_backup_object_key text,
    latest_backup_digest text check (
        latest_backup_digest is null
        or latest_backup_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    latest_backup_size_bytes bigint check (latest_backup_size_bytes > 0),
    latest_backup_refs_digest text check (
        latest_backup_refs_digest is null
        or latest_backup_refs_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    latest_backup_created_at timestamptz,
    updated_at timestamptz not null,
    primary key (organization_id, asset_id),
    foreign key (organization_id, asset_id)
        references assets (organization_id, id),
    check (
        (
            write_lease_id is null
            and write_lease_operation is null
            and write_lease_actor_id is null
            and write_lease_request_id is null
            and write_leased_until is null
            and not write_lease_recovering
        )
        or (
            write_lease_id is not null
            and write_lease_operation is not null
            and write_lease_actor_id is not null
            and write_lease_request_id is not null
            and write_leased_until is not null
        )
    ),
    check (write_lease_id is null or write_cleanup_lease_id is null),
    check (
        (
            latest_backup_object_key is null
            and latest_backup_digest is null
            and latest_backup_size_bytes is null
            and latest_backup_refs_digest is null
            and latest_backup_created_at is null
        )
        or (
            length(latest_backup_object_key) between 1 and 4096
            and latest_backup_digest is not null
            and latest_backup_size_bytes is not null
            and latest_backup_refs_digest is not null
            and latest_backup_created_at is not null
        )
    )
);

create index asset_git_repository_write_leases_idx
    on asset_git_repository_controls (write_leased_until, organization_id, asset_id)
    where write_lease_id is not null;

comment on table asset_git_repository_controls is
    'A0.2 PostgreSQL authority for hosted Git quotas, single-writer leases, same-lease rollback/cleanup recovery, observed usage, and latest immutable backup identity';
