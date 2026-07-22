alter table build_runs
    add constraint build_runs_workload_handoff_identity_unique
    unique (
        organization_id,
        project_id,
        environment_id,
        id,
        source_revision_id
    );

alter table workloads
    add constraint workloads_full_identity_unique
    unique (organization_id, project_id, environment_id, id);

alter table workload_revisions
    add column external_build_organization_id uuid,
    add column external_build_project_id uuid,
    add column external_build_environment_id uuid,
    add column external_source_revision_id uuid,
    add column external_build_run_id uuid,
    add constraint workload_revisions_external_build_shape_check check (
        (
            external_build_organization_id is null
            and external_build_project_id is null
            and external_build_environment_id is null
            and external_source_revision_id is null
            and external_build_run_id is null
        )
        or (
            resolution_state = 'resolved'
            and external_build_organization_id is not null
            and external_build_project_id is not null
            and external_build_environment_id is not null
            and external_source_revision_id is not null
            and external_build_run_id is not null
        )
    ),
    add constraint workload_revisions_external_build_fk foreign key (
        external_build_organization_id,
        external_build_project_id,
        external_build_environment_id,
        external_build_run_id,
        external_source_revision_id
    ) references build_runs (
        organization_id,
        project_id,
        environment_id,
        id,
        source_revision_id
    ),
    add constraint workload_revisions_external_build_scope_fk foreign key (
        external_build_organization_id,
        external_build_project_id,
        external_build_environment_id,
        workload_id
    ) references workloads (
        organization_id,
        project_id,
        environment_id,
        id
    );

create index workload_revisions_external_build_idx
    on workload_revisions (
        external_build_organization_id,
        external_source_revision_id,
        external_build_run_id,
        id
    )
    where external_build_run_id is not null;
