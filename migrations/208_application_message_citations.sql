create table application_message_citations (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    end_user_id uuid not null,
    invocation_id uuid not null,
    message_id uuid not null,
    message_kind text not null check (
        message_kind in ('answer', 'final_output')
    ),
    knowledge_base_id uuid not null,
    knowledge_base_revision_id uuid not null,
    knowledge_document_id uuid not null,
    knowledge_chunk_id uuid not null,
    excerpt text
        check (
            excerpt is null
            or (
                octet_length(excerpt) between 1 and 8192
                and position(E'\000' in excerpt) = 0
            )
        ),
    excerpt_digest text not null
        check (excerpt_digest ~ '^sha256:[0-9a-f]{64}$'),
    id uuid not null,
    created_at timestamptz not null,
    primary key (organization_id, application_id, id),
    unique (organization_id, project_id, application_id, id),
    foreign key (
        organization_id,
        project_id,
        application_id,
        session_id,
        application_release_id,
        application_release_digest
    ) references application_sessions (
        organization_id,
        project_id,
        application_id,
        id,
        application_release_id,
        application_release_digest
    ),
    foreign key (
        organization_id,
        project_id,
        application_id,
        end_user_id
    ) references application_end_users (
        organization_id,
        project_id,
        application_id,
        id
    ),
    foreign key (
        organization_id,
        project_id,
        application_id,
        invocation_id
    ) references application_invocations (
        organization_id,
        project_id,
        application_id,
        id
    ),
    foreign key (
        organization_id,
        project_id,
        application_id,
        message_id
    ) references application_messages (
        organization_id,
        project_id,
        application_id,
        id
    ),
    foreign key (
        organization_id,
        knowledge_base_id
    ) references knowledge_bases (
        organization_id,
        knowledge_base_id
    ),
    foreign key (
        organization_id,
        knowledge_base_revision_id
    ) references knowledge_base_revisions (
        organization_id,
        revision_id
    ),
    foreign key (
        organization_id,
        knowledge_document_id
    ) references knowledge_documents (
        organization_id,
        document_id
    ),
    foreign key (
        organization_id,
        knowledge_chunk_id
    ) references knowledge_chunks (
        organization_id,
        chunk_id
    )
);

create index application_message_citations_session_idx
    on application_message_citations (
        organization_id,
        project_id,
        application_id,
        session_id,
        created_at,
        id
    );

create trigger application_message_citations_immutable
before update or delete on application_message_citations
for each row execute function reject_application_session_child_mutation();

comment on table application_message_citations is
    'Immutable Applications-owned Answer/FinalOutput message citations; exact create replays by primary key without rewriting ApplicationMessage sequences';

