create table application_message_variants (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    end_user_id uuid not null,
    invocation_id uuid not null,
    source_message_id uuid not null,
    source_message_kind text not null check (
        source_message_kind in ('answer', 'final_output')
    ),
    id uuid not null,
    instruction jsonb
        check (
            instruction is null
            or (
                jsonb_typeof(instruction) = 'object'
                and octet_length(instruction::text) <= 4096
            )
        ),
    instruction_digest text not null
        check (instruction_digest ~ '^sha256:[0-9a-f]{64}$'),
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
        source_message_id
    ) references application_messages (
        organization_id,
        project_id,
        application_id,
        id
    )
);

create index application_message_variants_session_idx
    on application_message_variants (
        organization_id,
        project_id,
        application_id,
        session_id,
        created_at,
        id
    );

create trigger application_message_variants_immutable
before update or delete on application_message_variants
for each row execute function reject_application_session_child_mutation();

comment on table application_message_variants is
    'Immutable Applications-owned More Like This variants; exact create replays by primary key without rewriting ApplicationMessage sequences';
