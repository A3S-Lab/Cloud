alter table asset_releases
    add column agent_manifest_identity text,
    add column agent_manifest_acl text,
    add column agent_manifest_archive_digest text,
    add column agent_manifest_archive_size_bytes bigint,
    add column agent_manifest_source_content_digest text,
    add constraint asset_releases_agent_manifest_identity_check check (
        agent_manifest_identity is null
        or agent_manifest_identity ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint asset_releases_agent_manifest_acl_check check (
        agent_manifest_acl is null
        or octet_length(agent_manifest_acl) between 1 and 65536
    ),
    add constraint asset_releases_agent_manifest_archive_digest_check check (
        agent_manifest_archive_digest is null
        or agent_manifest_archive_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint asset_releases_agent_manifest_archive_size_check check (
        agent_manifest_archive_size_bytes is null
        or agent_manifest_archive_size_bytes > 0
    ),
    add constraint asset_releases_agent_manifest_source_digest_check check (
        agent_manifest_source_content_digest is null
        or agent_manifest_source_content_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint asset_releases_agent_manifest_shape_check check (
        (
            agent_manifest_identity is null
            and agent_manifest_acl is null
            and agent_manifest_archive_digest is null
            and agent_manifest_archive_size_bytes is null
            and agent_manifest_source_content_digest is null
        )
        or (
            state in ('published', 'yanked')
            and artifact_kind = 'oci_service'
            and build_run_id is not null
            and provenance_digest is not null
            and agent_manifest_identity is not null
            and agent_manifest_acl is not null
            and agent_manifest_archive_digest is not null
            and agent_manifest_archive_size_bytes is not null
            and agent_manifest_source_content_digest is not null
        )
    );

comment on column asset_releases.agent_manifest_acl is
    'Exact canonical a3s.code.agent-release.v1 bytes retained for immutable Agent deployment.';

comment on column asset_releases.agent_manifest_archive_digest is
    'Digest of the deterministic directory archive mounted read-only at /app/.a3s.';

alter table workload_revisions
    add column agent_release_contract jsonb,
    add constraint workload_revisions_agent_release_contract_shape_check check (
        agent_release_contract is null
        or (
            agent_organization_id is not null
            and agent_asset_id is not null
            and agent_asset_release_id is not null
            and agent_build_run_id is not null
            and jsonb_typeof(agent_release_contract) = 'object'
            and pg_column_size(agent_release_contract) between 1 and 262144
        )
    );

comment on column workload_revisions.agent_release_contract is
    'Exact canonical Agent release contract used to derive Runtime process, probes, storage, and manifest mount.';
