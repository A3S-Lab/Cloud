create table application_feedbacks (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    end_user_id uuid not null,
    source_message_id uuid,
    id uuid not null,
    rating text not null check (rating in ('positive', 'negative')),
    comment text
        check (comment is null or char_length(comment) between 1 and 4096),
    content_digest text not null
        check (content_digest ~ '^sha256:[0-9a-f]{64}$'),
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
        source_message_id
    ) references application_messages (
        organization_id,
        project_id,
        application_id,
        id
    )
);

create index application_feedbacks_session_idx
    on application_feedbacks (
        organization_id,
        project_id,
        application_id,
        session_id,
        created_at,
        id
    );

create table application_annotations (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    end_user_id uuid not null,
    source_message_id uuid,
    id uuid not null,
    content jsonb not null
        check (octet_length(content::text) <= 262144),
    content_digest text not null
        check (content_digest ~ '^sha256:[0-9a-f]{64}$'),
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
        source_message_id
    ) references application_messages (
        organization_id,
        project_id,
        application_id,
        id
    )
);

create index application_annotations_session_idx
    on application_annotations (
        organization_id,
        project_id,
        application_id,
        session_id,
        created_at,
        id
    );

create trigger application_feedbacks_immutable
before update or delete on application_feedbacks
for each row execute function reject_application_session_child_mutation();

create trigger application_annotations_immutable
before update or delete on application_annotations
for each row execute function reject_application_session_child_mutation();

comment on table application_feedbacks is
    'Immutable Applications-owned session feedback; exact create replays by primary key without rewriting ApplicationMessage sequences';

comment on table application_annotations is
    'Immutable Applications-owned session annotations; exact create replays by primary key without Annotation Reply matching';
