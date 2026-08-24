comment on column asset_releases.build_run_id is
    'Exact Artifacts-owned BuildRun identity admitted from a versioned HostedBuildOutcome fact; this relational identity guard grants no Artifacts write authority over Assets';

comment on constraint asset_releases_hosted_build_foreign_key on asset_releases is
    'Relational identity guard only; hosted release lifecycle transitions remain Assets-owned and are driven by the versioned Artifacts Outbox fact';

comment on constraint build_runs_asset_release_foreign_key on build_runs is
    'Relational identity guard only; Artifacts receives an immutable AssetRelease subject identity and never owns Asset release lifecycle state';

comment on constraint build_runs_hosted_release_publication_identity_unique on build_runs is
    'Supports exact tenant, AssetRelease, and BuildRun identity validation without granting either bounded context foreign write authority';
