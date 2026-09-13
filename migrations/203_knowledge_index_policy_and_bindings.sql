-- K0.1-C11: durable KnowledgeIndexRevision, KnowledgeRetrievalPolicyRevision,
-- and ExternalKnowledgeBinding catalogs. ACL is the only persisted semantic
-- source. No search index, provider client, authorization surface, or public
-- REST/MCP interface.

create table knowledge_index_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    knowledge_base_revision_id uuid not null,
    index_revision_id uuid not null,
    index_digest text not null
        check (index_digest ~ '^sha256:[0-9a-f]{64}$'),
    index_acl text not null
        check (octet_length(index_acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, index_revision_id),
    unique (index_revision_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, knowledge_base_revision_id)
        references knowledge_base_revisions (organization_id, revision_id)
);

create index knowledge_index_revisions_base_revision_idx
    on knowledge_index_revisions (organization_id, knowledge_base_revision_id, index_revision_id);

create index knowledge_index_revisions_scope_idx
    on knowledge_index_revisions (organization_id, project_id, index_revision_id);

create table knowledge_retrieval_policy_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    knowledge_base_revision_id uuid not null,
    policy_revision_id uuid not null,
    policy_digest text not null
        check (policy_digest ~ '^sha256:[0-9a-f]{64}$'),
    policy_acl text not null
        check (octet_length(policy_acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, policy_revision_id),
    unique (policy_revision_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, knowledge_base_revision_id)
        references knowledge_base_revisions (organization_id, revision_id)
);

create index knowledge_retrieval_policy_revisions_base_revision_idx
    on knowledge_retrieval_policy_revisions (
        organization_id, knowledge_base_revision_id, policy_revision_id
    );

create index knowledge_retrieval_policy_revisions_scope_idx
    on knowledge_retrieval_policy_revisions (organization_id, project_id, policy_revision_id);

create table external_knowledge_bindings (
    organization_id uuid not null,
    project_id uuid not null,
    knowledge_base_id uuid not null,
    binding_id uuid not null,
    binding_digest text not null
        check (binding_digest ~ '^sha256:[0-9a-f]{64}$'),
    binding_acl text not null
        check (octet_length(binding_acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, binding_id),
    unique (binding_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, knowledge_base_id)
        references knowledge_bases (organization_id, knowledge_base_id)
);

create index external_knowledge_bindings_base_idx
    on external_knowledge_bindings (organization_id, knowledge_base_id, binding_id);

create index external_knowledge_bindings_scope_idx
    on external_knowledge_bindings (organization_id, project_id, binding_id);

create function enforce_knowledge_index_revision_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'UPDATE' or tg_op = 'DELETE' then
        raise exception 'Knowledge index revisions are immutable';
    end if;
    return new;
end
$$;

create trigger knowledge_index_revisions_immutable
before update or delete on knowledge_index_revisions
for each row execute function enforce_knowledge_index_revision_immutability();

create function enforce_knowledge_retrieval_policy_revision_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'UPDATE' or tg_op = 'DELETE' then
        raise exception 'Knowledge retrieval policy revisions are immutable';
    end if;
    return new;
end
$$;

create trigger knowledge_retrieval_policy_revisions_immutable
before update or delete on knowledge_retrieval_policy_revisions
for each row execute function enforce_knowledge_retrieval_policy_revision_immutability();

create function enforce_external_knowledge_binding_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'UPDATE' or tg_op = 'DELETE' then
        raise exception 'External Knowledge bindings are immutable';
    end if;
    return new;
end
$$;

create trigger external_knowledge_bindings_immutable
before update or delete on external_knowledge_bindings
for each row execute function enforce_external_knowledge_binding_immutability();
