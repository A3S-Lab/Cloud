create table artifact_build_candidates (
    organization_id uuid not null,
    subject_kind text not null check (
        subject_kind in ('external_source_revision', 'asset_release')
    ),
    subject_id uuid not null,
    project_id uuid,
    environment_id uuid,
    source_revision_id uuid,
    asset_id uuid,
    asset_release_id uuid,
    repository_identity text,
    commit_sha text not null check (commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    owner_input_digest text not null check (
        owner_input_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    requested_at timestamptz not null,
    primary key (organization_id, subject_kind, subject_id),
    check (
        (
            subject_kind = 'external_source_revision'
            and subject_id = source_revision_id
            and project_id is not null
            and environment_id is not null
            and source_revision_id is not null
            and asset_id is null
            and asset_release_id is null
            and repository_identity is not null
            and btrim(repository_identity) <> ''
            and length(repository_identity) between 1 and 2048
        )
        or (
            subject_kind = 'asset_release'
            and subject_id = asset_release_id
            and project_id is null
            and environment_id is null
            and source_revision_id is null
            and asset_id is not null
            and asset_release_id is not null
            and repository_identity is null
        )
    )
);

create index artifact_build_candidates_pending_idx
    on artifact_build_candidates (requested_at, subject_kind, subject_id);

comment on table artifact_build_candidates is
    'Artifacts-owned immutable projection of owner-published build request facts; it is not a queue or lifecycle state machine';

comment on column artifact_build_candidates.requested_at is
    'Owner fact occurrence time used for deterministic BuildRun admission ordering';

comment on column artifact_build_candidates.owner_input_digest is
    'Exact owner fact recipe digest for Source revisions or manifest digest for hosted Asset releases; retained to reject semantic replay drift';

-- One-time upgrade seed for facts committed before the Artifacts projector
-- existed. Drain pre-152 Assets writers before applying this migration because
-- they do not emit asset.hosted-build.requested@1. Runtime candidate discovery
-- reads only this Artifacts-owned table.
insert into artifact_build_candidates (
    organization_id,
    subject_kind,
    subject_id,
    project_id,
    environment_id,
    source_revision_id,
    asset_id,
    asset_release_id,
    repository_identity,
    commit_sha,
    owner_input_digest,
    requested_at
)
select
    r.organization_id,
    'external_source_revision',
    r.id,
    r.project_id,
    r.environment_id,
    r.id,
    null,
    null,
    r.repository_identity,
    r.commit_sha,
    r.recipe_digest,
    r.accepted_at
from external_source_revisions r
on conflict (organization_id, subject_kind, subject_id) do nothing;

insert into artifact_build_candidates (
    organization_id,
    subject_kind,
    subject_id,
    project_id,
    environment_id,
    source_revision_id,
    asset_id,
    asset_release_id,
    repository_identity,
    commit_sha,
    owner_input_digest,
    requested_at
)
select
    r.organization_id,
    'asset_release',
    r.id,
    null,
    null,
    null,
    r.asset_id,
    r.id,
    null,
    r.commit_sha,
    r.manifest_digest,
    r.updated_at
from asset_releases r
join assets a
    on a.organization_id = r.organization_id
    and a.id = r.asset_id
where r.state = 'draft'
    and a.state = 'active'
    and a.kind in ('agent', 'mcp')
on conflict (organization_id, subject_kind, subject_id) do nothing;
