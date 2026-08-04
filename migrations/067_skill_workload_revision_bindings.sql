alter table asset_releases
    add constraint asset_releases_skill_workload_identity_unique
    unique (
        organization_id,
        asset_id,
        id,
        artifact_digest,
        artifact_media_type,
        artifact_size_bytes
    );

create table workload_revision_skill_bindings (
    organization_id uuid not null,
    workload_id uuid not null,
    revision_id uuid not null,
    asset_id uuid not null,
    asset_release_id uuid not null,
    artifact_digest text not null,
    artifact_size_bytes bigint not null check (artifact_size_bytes > 0),
    artifact_media_type text generated always as
        ('application/vnd.a3s.skill.bundle.v1+tar') stored,
    primary key (revision_id, asset_id),
    unique (revision_id, asset_release_id),
    foreign key (organization_id, workload_id)
        references workloads (organization_id, id),
    foreign key (workload_id, revision_id)
        references workload_revisions (workload_id, id),
    foreign key (
        organization_id,
        asset_id,
        asset_release_id,
        artifact_digest,
        artifact_media_type,
        artifact_size_bytes
    )
        references asset_releases (
            organization_id,
            asset_id,
            id,
            artifact_digest,
            artifact_media_type,
            artifact_size_bytes
        ),
    check (artifact_digest ~ '^sha256:[0-9a-f]{64}$')
);

create index workload_revision_skill_bindings_release_idx
    on workload_revision_skill_bindings (
        organization_id,
        asset_id,
        asset_release_id,
        revision_id
    );

comment on table workload_revision_skill_bindings is
    'Exact immutable Skill bundle inputs mounted read-only into Agent Workload revisions';
