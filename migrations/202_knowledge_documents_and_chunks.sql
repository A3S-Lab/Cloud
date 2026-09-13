-- K0.1-C5: durable KnowledgeDocument and KnowledgeChunk catalogs.
-- ACL is the only persisted semantic source. No search index, provider client,
-- ingestion worker, authorization surface, or public REST/MCP interface.

create table knowledge_documents (
    organization_id uuid not null,
    project_id uuid not null,
    knowledge_base_id uuid not null,
    knowledge_base_revision_id uuid not null,
    document_id uuid not null,
    document_digest text not null
        check (document_digest ~ '^sha256:[0-9a-f]{64}$'),
    document_acl text not null
        check (octet_length(document_acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, document_id),
    unique (document_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, knowledge_base_id)
        references knowledge_bases (organization_id, knowledge_base_id),
    foreign key (organization_id, knowledge_base_revision_id)
        references knowledge_base_revisions (organization_id, revision_id)
);

create index knowledge_documents_base_idx
    on knowledge_documents (organization_id, knowledge_base_id, document_id);

create index knowledge_documents_scope_idx
    on knowledge_documents (organization_id, project_id, document_id);

create table knowledge_chunks (
    organization_id uuid not null,
    project_id uuid not null,
    document_id uuid not null,
    chunk_id uuid not null,
    chunk_digest text not null
        check (chunk_digest ~ '^sha256:[0-9a-f]{64}$'),
    chunk_acl text not null
        check (octet_length(chunk_acl) between 1 and 65536),
    created_at timestamptz not null,
    primary key (organization_id, chunk_id),
    unique (chunk_id),
    foreign key (organization_id, project_id)
        references projects (organization_id, id),
    foreign key (organization_id, document_id)
        references knowledge_documents (organization_id, document_id)
);

create index knowledge_chunks_document_idx
    on knowledge_chunks (organization_id, document_id, chunk_id);

create function enforce_knowledge_document_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'UPDATE' or tg_op = 'DELETE' then
        raise exception 'Knowledge documents are immutable';
    end if;
    return new;
end
$$;

create trigger knowledge_documents_immutable
before update or delete on knowledge_documents
for each row execute function enforce_knowledge_document_immutability();

create function enforce_knowledge_chunk_immutability()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'UPDATE' or tg_op = 'DELETE' then
        raise exception 'Knowledge chunks are immutable';
    end if;
    return new;
end
$$;

create trigger knowledge_chunks_immutable
before update or delete on knowledge_chunks
for each row execute function enforce_knowledge_chunk_immutability();
