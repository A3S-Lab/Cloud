create table assets (
    organization_id uuid not null references organizations(id),
    id uuid not null,
    name text not null,
    name_key text not null,
    kind text not null check (kind in ('agent', 'mcp', 'skill')),
    state text not null check (state in ('active', 'archived')),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    archived_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, name_key),
    check (updated_at >= created_at),
    check (
        (state = 'active' and archived_at is null)
        or (
            state = 'archived'
            and archived_at is not null
            and archived_at = updated_at
        )
    )
);

create index assets_organization_state_idx
    on assets (organization_id, state, created_at, id);

create table asset_releases (
    organization_id uuid not null,
    asset_id uuid not null,
    id uuid not null,
    version text not null check (length(version) between 1 and 128),
    state text not null check (state in ('draft', 'published', 'yanked')),
    commit_sha text not null check (commit_sha ~ '^[0-9a-f]{40}([0-9a-f]{24})?$'),
    manifest_digest text not null check (manifest_digest ~ '^sha256:[0-9a-f]{64}$'),
    artifact_kind text check (artifact_kind in ('oci_service', 'skill_bundle')),
    artifact_digest text check (
        artifact_digest is null
        or artifact_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    artifact_media_type text,
    artifact_size_bytes bigint check (artifact_size_bytes > 0),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    published_at timestamptz,
    yanked_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, asset_id, version),
    foreign key (organization_id, asset_id)
        references assets (organization_id, id),
    check (updated_at >= created_at),
    check (
        (
            artifact_kind is null
            and artifact_digest is null
            and artifact_media_type is null
            and artifact_size_bytes is null
        )
        or (
            artifact_kind is not null
            and artifact_digest is not null
            and artifact_media_type is not null
            and artifact_size_bytes is not null
        )
    ),
    check (
        artifact_kind is null
        or (
            artifact_kind = 'oci_service'
            and artifact_media_type in (
                'application/vnd.oci.image.index.v1+json',
                'application/vnd.oci.image.manifest.v1+json'
            )
        )
        or (
            artifact_kind = 'skill_bundle'
            and artifact_media_type = 'application/vnd.a3s.skill.bundle.v1+tar'
        )
    ),
    check (
        (
            state = 'draft'
            and artifact_kind is null
            and published_at is null
            and yanked_at is null
        )
        or (
            state = 'published'
            and artifact_kind is not null
            and published_at = updated_at
            and yanked_at is null
        )
        or (
            state = 'yanked'
            and artifact_kind is not null
            and published_at is not null
            and published_at <= yanked_at
            and yanked_at = updated_at
        )
    )
);

create index asset_releases_asset_state_idx
    on asset_releases (organization_id, asset_id, state, created_at, id);

comment on table assets is
    'A0 hosted Agent, MCP, and Skill identities; repositories are addressed by immutable asset ID';

comment on table asset_releases is
    'A0 immutable commit, manifest, and typed artifact release identities';
