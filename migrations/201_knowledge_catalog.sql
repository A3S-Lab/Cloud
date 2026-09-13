-- K0.1-C4a: durable KnowledgeBase and KnowledgePipelineRelease catalogs.
-- Revision/release ACL is the only persisted semantic source. No search index,
-- provider client, DAG engine, worker queue, or public surface is introduced.

create table knowledge_bases (
    organization_id uuid not null,
    project_id uuid not null,
    knowledge_base_id uuid not null,
    current_revision_id uuid not null,
    current_generation bigint not null check (current_generation > 0),
    current_revision_digest text not null
        check (current_revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, knowledge_base_id),
    unique (knowledge_base_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    check (created_at <= updated_at)
);

create table knowledge_base_revisions (
    organization_id uuid not null,
    knowledge_base_id uuid not null,
    revision_id uuid not null,
    generation bigint not null check (generation > 0),
    parent_revision_id uuid,
    parent_digest text,
    revision_digest text not null
        check (revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    revision_acl text not null
        check (octet_length(revision_acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, revision_id),
    unique (revision_id),
    unique (organization_id, knowledge_base_id, generation),
    foreign key (organization_id, knowledge_base_id)
        references knowledge_bases (organization_id, knowledge_base_id),
    foreign key (organization_id, parent_revision_id)
        references knowledge_base_revisions (organization_id, revision_id),
    check (
        (generation = 1 and parent_revision_id is null and parent_digest is null)
        or (generation > 1 and parent_revision_id is not null and parent_digest is not null)
    ),
    check (parent_digest is null or parent_digest ~ '^sha256:[0-9a-f]{64}$')
);

alter table knowledge_bases
    add constraint knowledge_bases_current_revision_fk
    foreign key (organization_id, current_revision_id)
    references knowledge_base_revisions (organization_id, revision_id)
    deferrable initially deferred;

create index knowledge_bases_scope_idx
    on knowledge_bases (organization_id, project_id, knowledge_base_id);

create index knowledge_base_revisions_history_idx
    on knowledge_base_revisions (organization_id, knowledge_base_id, generation, revision_id);

create table knowledge_pipelines (
    organization_id uuid not null,
    project_id uuid not null,
    pipeline_id uuid not null,
    current_release_id uuid not null,
    current_release_digest text not null
        check (current_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (organization_id, pipeline_id),
    unique (pipeline_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    check (created_at <= updated_at)
);

create table knowledge_pipeline_releases (
    organization_id uuid not null,
    pipeline_id uuid not null,
    release_id uuid not null,
    release_digest text not null
        check (release_digest ~ '^sha256:[0-9a-f]{64}$'),
    release_acl text not null
        check (octet_length(release_acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, release_id),
    unique (release_id),
    foreign key (organization_id, pipeline_id)
        references knowledge_pipelines (organization_id, pipeline_id)
);

alter table knowledge_pipelines
    add constraint knowledge_pipelines_current_release_fk
    foreign key (organization_id, current_release_id)
    references knowledge_pipeline_releases (organization_id, release_id)
    deferrable initially deferred;

create index knowledge_pipelines_scope_idx
    on knowledge_pipelines (organization_id, project_id, pipeline_id);

create function enforce_knowledge_base_head()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Knowledge bases cannot be deleted';
    end if;
    if new.organization_id <> old.organization_id
       or new.project_id <> old.project_id
       or new.knowledge_base_id <> old.knowledge_base_id
       or new.created_at <> old.created_at then
        raise exception 'Knowledge base identity and creation time are immutable';
    end if;
    if new.current_generation <> old.current_generation + 1
       or new.current_revision_id = old.current_revision_id
       or new.current_revision_digest = old.current_revision_digest
       or new.updated_at < old.updated_at then
        raise exception 'Knowledge base head must advance exactly one immutable revision';
    end if;
    if not exists (
        select 1
        from knowledge_base_revisions revision
        where revision.organization_id = new.organization_id
          and revision.knowledge_base_id = new.knowledge_base_id
          and revision.revision_id = new.current_revision_id
          and revision.generation = new.current_generation
          and revision.revision_digest = new.current_revision_digest
    ) then
        raise exception 'Knowledge base head must reference its exact revision';
    end if;
    return new;
end
$$;

create trigger knowledge_bases_validate_head
before update or delete on knowledge_bases
for each row execute function enforce_knowledge_base_head();

create function enforce_knowledge_base_revision_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'UPDATE' or tg_op = 'DELETE' then
        raise exception 'Knowledge base revisions are immutable';
    end if;
    return new;
end
$$;

create trigger knowledge_base_revisions_immutable
before update or delete on knowledge_base_revisions
for each row execute function enforce_knowledge_base_revision_immutability();

create function enforce_knowledge_pipeline_head()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Knowledge pipelines cannot be deleted';
    end if;
    if new.organization_id <> old.organization_id
       or new.project_id <> old.project_id
       or new.pipeline_id <> old.pipeline_id
       or new.created_at <> old.created_at then
        raise exception 'Knowledge pipeline identity and creation time are immutable';
    end if;
    if new.current_release_id = old.current_release_id
       or new.current_release_digest = old.current_release_digest
       or new.updated_at < old.updated_at then
        raise exception 'Knowledge pipeline head must advance to a new immutable release';
    end if;
    if not exists (
        select 1
        from knowledge_pipeline_releases release
        where release.organization_id = new.organization_id
          and release.pipeline_id = new.pipeline_id
          and release.release_id = new.current_release_id
          and release.release_digest = new.current_release_digest
    ) then
        raise exception 'Knowledge pipeline head must reference its exact release';
    end if;
    return new;
end
$$;

create trigger knowledge_pipelines_validate_head
before update or delete on knowledge_pipelines
for each row execute function enforce_knowledge_pipeline_head();

create function enforce_knowledge_pipeline_release_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'UPDATE' or tg_op = 'DELETE' then
        raise exception 'Knowledge pipeline releases are immutable';
    end if;
    return new;
end
$$;

create trigger knowledge_pipeline_releases_immutable
before update or delete on knowledge_pipeline_releases
for each row execute function enforce_knowledge_pipeline_release_immutability();
