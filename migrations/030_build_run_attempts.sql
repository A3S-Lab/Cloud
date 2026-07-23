alter table build_runs
    drop constraint build_runs_organization_id_source_revision_id_key;

alter table build_runs
    add column attempt integer not null default 1,
    add column retry_of_build_run_id uuid,
    add constraint build_runs_attempt_positive check (attempt > 0),
    add constraint build_runs_retry_shape check (
        (attempt = 1 and retry_of_build_run_id is null)
        or (attempt > 1 and retry_of_build_run_id is not null)
    ),
    add constraint build_runs_source_attempt_unique
        unique (organization_id, source_revision_id, attempt),
    add constraint build_runs_retry_parent_unique
        unique (organization_id, retry_of_build_run_id),
    add constraint build_runs_retry_parent_foreign_key
        foreign key (organization_id, retry_of_build_run_id)
        references build_runs (organization_id, id);

alter table build_runs
    alter column attempt drop default;

create index build_runs_source_attempt_idx
    on build_runs (organization_id, source_revision_id, attempt desc);
