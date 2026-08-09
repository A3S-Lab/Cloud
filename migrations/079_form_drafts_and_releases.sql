create table form_drafts (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    name text not null check (char_length(name) between 1 and 120),
    name_key text not null check (char_length(name_key) between 1 and 120),
    description text not null check (char_length(description) <= 4096),
    canonical_document_json text not null
        check (octet_length(canonical_document_json) between 1 and 4194304),
    draft_digest text not null check (draft_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    latest_release_id uuid,
    created_by uuid not null references identity_principals(id),
    updated_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, id),
    constraint form_drafts_project_name_unique
        unique (organization_id, project_id, name_key),
    unique (organization_id, project_id, id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    check (updated_at >= created_at)
);

create table form_releases (
    organization_id uuid not null,
    project_id uuid not null,
    form_id uuid not null,
    id uuid not null,
    revision bigint not null check (revision > 0),
    source_draft_version bigint not null check (source_draft_version > 0),
    name text not null check (char_length(name) between 1 and 120),
    description text not null check (char_length(description) <= 4096),
    normalized_document_json text not null
        check (octet_length(normalized_document_json) between 1 and 4194304),
    form_plan_json text not null
        check (octet_length(form_plan_json) between 1 and 16777216),
    compiler_revision text not null
        check (compiler_revision = 'a3s-form-core@0.1.0'),
    schema_profile text not null
        check (schema_profile = 'a3s.dev/form-schema-profile/1'),
    content_digest text not null check (content_digest ~ '^sha256:[0-9a-f]{64}$'),
    published_by uuid not null references identity_principals(id),
    published_at timestamptz not null,
    primary key (organization_id, form_id, id),
    constraint form_releases_revision_unique
        unique (organization_id, form_id, revision),
    constraint form_releases_source_draft_version_unique
        unique (organization_id, form_id, source_draft_version),
    unique (organization_id, project_id, form_id, id),
    constraint form_releases_draft_fk
        foreign key (organization_id, project_id, form_id)
        references form_drafts (organization_id, project_id, id)
);

alter table form_drafts
    add constraint form_drafts_latest_release_fk
    foreign key (organization_id, id, latest_release_id)
    references form_releases (organization_id, form_id, id)
    deferrable initially deferred;

create index form_drafts_project_updated_idx
    on form_drafts (organization_id, project_id, updated_at desc, id);

create index form_releases_lineage_idx
    on form_releases (organization_id, form_id, revision desc, id);

create function reject_form_release_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Form releases are immutable';
end
$$;

create trigger form_releases_immutable
before update or delete on form_releases
for each row execute function reject_form_release_mutation();

update api_tokens
set scopes = scopes || '["form:write"]'::jsonb
where (scopes ? 'platform:write' or scopes ? 'project:write')
  and not scopes ? 'form:write';

comment on table form_drafts is
    'Mutable project-scoped Form aggregate heads containing exact Form-owned canonical draft JSON';

comment on table form_releases is
    'Immutable published Form content compiled by the exact pinned A3S Form semantic core';
