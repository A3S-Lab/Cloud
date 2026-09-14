create table application_message_file_references (
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
        message_kind in ('input')
    ),
    user_file_id uuid not null,
    content_digest text not null
        check (content_digest ~ '^sha256:[0-9a-f]{64}$'),
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
        user_file_id
    ) references user_files (
        organization_id,
        id
    )
);

create index application_message_file_references_session_idx
    on application_message_file_references (
        organization_id,
        project_id,
        application_id,
        session_id,
        created_at,
        id
    );

create trigger application_message_file_references_immutable
before update or delete on application_message_file_references
for each row execute function reject_application_session_child_mutation();

comment on table application_message_file_references is
    'Immutable Applications-owned Input message file references; exact create replays by primary key without rewriting ApplicationMessage sequences';
