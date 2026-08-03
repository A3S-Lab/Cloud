alter table asset_releases
    add column build_run_id uuid,
    add column provenance_digest text;

do $$
begin
    if exists (
        select 1
        from asset_releases
        where state in ('published', 'yanked')
          and artifact_kind = 'oci_service'
    ) then
        raise exception using
            message = 'migration 064 requires hosted OCI releases to remain draft until their BuildRun is finalized';
    end if;
end;
$$;

alter table build_runs
    add constraint build_runs_hosted_release_publication_identity_unique
    unique (organization_id, asset_id, asset_release_id, id);

alter table asset_releases
    add constraint asset_releases_provenance_digest_check check (
        provenance_digest is null
        or provenance_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    add constraint asset_releases_publication_provenance_shape_check check (
        (
            state = 'draft'
            and artifact_kind is null
            and build_run_id is null
            and provenance_digest is null
        )
        or (
            state in ('published', 'yanked')
            and artifact_kind = 'oci_service'
            and build_run_id is not null
            and provenance_digest is not null
        )
        or (
            state in ('published', 'yanked')
            and artifact_kind = 'skill_bundle'
            and build_run_id is null
            and provenance_digest is null
        )
    ),
    add constraint asset_releases_hosted_build_foreign_key foreign key (
        organization_id,
        asset_id,
        id,
        build_run_id
    ) references build_runs (
        organization_id,
        asset_id,
        asset_release_id,
        id
    );

comment on column asset_releases.build_run_id is
    'Exact successful hosted BuildRun that atomically published this Agent or MCP release';

comment on column asset_releases.provenance_digest is
    'SHA-256 identity of the verified SLSA provenance stored authoritatively on the linked BuildRun';
