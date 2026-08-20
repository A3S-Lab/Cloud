create table application_end_users (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    id uuid not null,
    audience text not null check (
        audience in ('project_members', 'authenticated_end_users', 'anonymous')
    ),
    linked_principal_id uuid references identity_principals(id),
    created_by uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (organization_id, application_id, id),
    unique (organization_id, project_id, application_id, id),
    foreign key (organization_id, project_id, application_id)
        references applications (organization_id, project_id, id),
    check (
        audience = 'project_members' and linked_principal_id is not null
        or audience = 'authenticated_end_users'
        or audience = 'anonymous' and linked_principal_id is null
    )
);

create table application_sessions (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_number bigint not null check (application_release_number > 0),
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    end_user_id uuid not null,
    id uuid not null,
    interaction_mode text not null check (
        interaction_mode in ('conversation', 'invocation')
    ),
    status text not null check (status in ('active', 'closed')),
    last_message_sequence bigint not null check (last_message_sequence >= 0),
    current_variable_revision_id uuid not null,
    current_variable_revision_number bigint not null
        check (current_variable_revision_number > 0),
    current_variable_digest text not null
        check (current_variable_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    closed_at timestamptz,
    primary key (organization_id, application_id, id),
    unique (organization_id, project_id, application_id, id),
    unique (
        organization_id,
        project_id,
        application_id,
        id,
        application_release_id,
        application_release_digest
    ),
    foreign key (organization_id, project_id, application_id, application_release_id)
        references application_releases (
            organization_id,
            project_id,
            application_id,
            id
        ),
    foreign key (organization_id, project_id, application_id, end_user_id)
        references application_end_users (
            organization_id,
            project_id,
            application_id,
            id
        ),
    check (
        aggregate_version = last_message_sequence
            + current_variable_revision_number
            + case when status = 'closed' then 1 else 0 end
    ),
    check (updated_at >= created_at),
    check (
        status = 'active' and closed_at is null
        or status = 'closed' and closed_at = updated_at
    )
);

create table application_invocations (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    id uuid not null,
    response_mode text not null check (
        response_mode in ('asynchronous', 'blocking', 'streaming')
    ),
    input jsonb not null check (
        jsonb_typeof(input) = 'object'
        and octet_length(input::text) <= 65536
    ),
    input_digest text not null check (input_digest ~ '^sha256:[0-9a-f]{64}$'),
    workflow_run_id uuid,
    status text not null check (
        status in ('requested', 'running', 'cancelling', 'succeeded', 'failed', 'cancelled')
    ),
    aggregate_version bigint not null check (aggregate_version > 0),
    requested_at timestamptz not null,
    updated_at timestamptz not null,
    completed_at timestamptz,
    primary key (organization_id, application_id, id),
    unique (organization_id, project_id, application_id, id),
    unique (
        organization_id,
        project_id,
        application_id,
        session_id,
        application_release_id,
        application_release_digest,
        id
    ),
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
    foreign key (organization_id, project_id, workflow_run_id)
        references workflow_runs (organization_id, project_id, id),
    check (updated_at >= requested_at),
    check ((status in ('succeeded', 'failed', 'cancelled')) = (completed_at is not null)),
    check (completed_at is null or completed_at = updated_at),
    check (status <> 'requested' or workflow_run_id is null),
    check (status not in ('running', 'succeeded', 'failed') or workflow_run_id is not null),
    check (
        status = 'requested' and workflow_run_id is null and aggregate_version = 1
        or status = 'running' and workflow_run_id is not null and aggregate_version = 2
        or status = 'cancelling' and workflow_run_id is null and aggregate_version = 2
        or status = 'cancelling' and workflow_run_id is not null and aggregate_version = 3
        or status in ('succeeded', 'failed')
            and workflow_run_id is not null
            and aggregate_version in (3, 4)
        or status = 'cancelled'
            and workflow_run_id is null
            and aggregate_version in (2, 3)
        or status = 'cancelled'
            and workflow_run_id is not null
            and aggregate_version in (3, 4)
    )
);

create unique index application_invocations_workflow_run_unique
    on application_invocations (organization_id, workflow_run_id)
    where workflow_run_id is not null;

create table application_messages (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    invocation_id uuid not null,
    id uuid not null,
    sequence bigint not null check (sequence > 0),
    kind text not null check (kind in ('input', 'answer', 'final_output')),
    content jsonb not null check (octet_length(content::text) <= 262144),
    content_digest text not null check (content_digest ~ '^sha256:[0-9a-f]{64}$'),
    workflow_run_id uuid,
    workflow_step_id text,
    workflow_attempt integer,
    workflow_effect_ordinal integer,
    created_at timestamptz not null,
    primary key (organization_id, application_id, id),
    unique (organization_id, project_id, application_id, id),
    unique (organization_id, application_id, session_id, sequence),
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
        session_id,
        application_release_id,
        application_release_digest,
        invocation_id
    ) references application_invocations (
        organization_id,
        project_id,
        application_id,
        session_id,
        application_release_id,
        application_release_digest,
        id
    ),
    check (
        kind = 'input'
            and workflow_run_id is null
            and workflow_step_id is null
            and workflow_attempt is null
            and workflow_effect_ordinal is null
        or kind in ('answer', 'final_output')
            and workflow_run_id is not null
            and workflow_step_id ~ '^[A-Za-z0-9_-]{1,96}$'
            and workflow_attempt > 0
            and workflow_effect_ordinal >= 0
    )
);

create unique index application_messages_final_output_unique
    on application_messages (organization_id, application_id, invocation_id)
    where kind = 'final_output';

create table application_conversation_variable_revisions (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    application_release_id uuid not null,
    application_release_digest text not null
        check (application_release_digest ~ '^sha256:[0-9a-f]{64}$'),
    session_id uuid not null,
    id uuid not null,
    revision_number bigint not null check (revision_number > 0),
    parent_revision_id uuid,
    parent_digest text check (parent_digest ~ '^sha256:[0-9a-f]{64}$'),
    values_json jsonb not null check (
        jsonb_typeof(values_json) = 'object'
        and octet_length(values_json::text) <= 262144
    ),
    values_digest text not null check (values_digest ~ '^sha256:[0-9a-f]{64}$'),
    workflow_run_id uuid,
    workflow_step_id text,
    workflow_attempt integer,
    workflow_effect_ordinal integer,
    created_at timestamptz not null,
    primary key (organization_id, application_id, session_id, id),
    unique (organization_id, project_id, application_id, session_id, id),
    unique (organization_id, application_id, session_id, revision_number),
    unique (organization_id, application_id, session_id, parent_revision_id),
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
        session_id,
        parent_revision_id
    ) references application_conversation_variable_revisions (
        organization_id,
        project_id,
        application_id,
        session_id,
        id
    ),
    check (
        revision_number = 1
            and parent_revision_id is null
            and parent_digest is null
            and workflow_run_id is null
            and workflow_step_id is null
            and workflow_attempt is null
            and workflow_effect_ordinal is null
        or revision_number > 1
            and parent_revision_id is not null
            and parent_digest is not null
            and parent_digest <> values_digest
            and workflow_run_id is not null
            and workflow_step_id ~ '^[A-Za-z0-9_-]{1,96}$'
            and workflow_attempt > 0
            and workflow_effect_ordinal >= 0
    )
);

alter table application_sessions
    add constraint application_sessions_variable_head_fk
    foreign key (
        organization_id,
        project_id,
        application_id,
        id,
        current_variable_revision_id
    ) references application_conversation_variable_revisions (
        organization_id,
        project_id,
        application_id,
        session_id,
        id
    )
    deferrable initially deferred;

create table application_workflow_effect_claims (
    organization_id uuid not null,
    project_id uuid not null,
    application_id uuid not null,
    session_id uuid not null,
    workflow_run_id uuid not null,
    workflow_step_id text not null check (
        workflow_step_id ~ '^[A-Za-z0-9_-]{1,96}$'
    ),
    workflow_attempt integer not null check (workflow_attempt > 0),
    workflow_effect_ordinal integer not null check (workflow_effect_ordinal >= 0),
    semantic_kind text not null check (
        semantic_kind in ('message_answer', 'message_final_output', 'conversation_variables')
    ),
    semantic_id uuid not null,
    primary key (
        organization_id,
        application_id,
        session_id,
        workflow_run_id,
        workflow_step_id,
        workflow_attempt,
        workflow_effect_ordinal
    ),
    unique (organization_id, application_id, semantic_kind, semantic_id),
    foreign key (organization_id, project_id, application_id, session_id)
        references application_sessions (
            organization_id,
            project_id,
            application_id,
            id
        )
);

create index application_sessions_end_user_updated_idx
    on application_sessions (
        organization_id,
        project_id,
        application_id,
        end_user_id,
        updated_at desc,
        id desc
    );

create index application_invocations_session_requested_idx
    on application_invocations (
        organization_id,
        application_id,
        session_id,
        requested_at desc,
        id desc
    );

create index application_messages_session_sequence_idx
    on application_messages (
        organization_id,
        application_id,
        session_id,
        sequence,
        id
    );

create function reject_application_session_child_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Application session semantic children are immutable';
end
$$;

create function validate_application_session_update()
returns trigger
language plpgsql
as $$
declare
    message_advanced boolean;
    variables_advanced boolean;
    session_closed boolean;
begin
    if new.organization_id is distinct from old.organization_id
       or new.project_id is distinct from old.project_id
       or new.application_id is distinct from old.application_id
       or new.application_release_id is distinct from old.application_release_id
       or new.application_release_number is distinct from old.application_release_number
       or new.application_release_digest is distinct from old.application_release_digest
       or new.end_user_id is distinct from old.end_user_id
       or new.id is distinct from old.id
       or new.interaction_mode is distinct from old.interaction_mode
       or new.created_at is distinct from old.created_at
       or old.status <> 'active'
       or new.aggregate_version <> old.aggregate_version + 1
       or new.updated_at < old.updated_at then
        raise exception 'Application session update is stale or changes immutable authority';
    end if;

    message_advanced := new.status = 'active'
        and new.closed_at is null
        and new.last_message_sequence = old.last_message_sequence + 1
        and new.current_variable_revision_id = old.current_variable_revision_id
        and new.current_variable_revision_number = old.current_variable_revision_number
        and new.current_variable_digest = old.current_variable_digest;
    variables_advanced := new.status = 'active'
        and new.closed_at is null
        and new.last_message_sequence = old.last_message_sequence
        and new.current_variable_revision_id <> old.current_variable_revision_id
        and new.current_variable_revision_number = old.current_variable_revision_number + 1
        and new.current_variable_digest <> old.current_variable_digest;
    session_closed := new.status = 'closed'
        and new.closed_at = new.updated_at
        and new.last_message_sequence = old.last_message_sequence
        and new.current_variable_revision_id = old.current_variable_revision_id
        and new.current_variable_revision_number = old.current_variable_revision_number
        and new.current_variable_digest = old.current_variable_digest;

    if (message_advanced::integer + variables_advanced::integer + session_closed::integer) <> 1 then
        raise exception 'Application session update must advance exactly one owned concern';
    end if;
    return new;
end
$$;

create function validate_application_invocation_update()
returns trigger
language plpgsql
as $$
declare
    valid_transition boolean;
begin
    if new.organization_id is distinct from old.organization_id
       or new.project_id is distinct from old.project_id
       or new.application_id is distinct from old.application_id
       or new.application_release_id is distinct from old.application_release_id
       or new.application_release_digest is distinct from old.application_release_digest
       or new.session_id is distinct from old.session_id
       or new.id is distinct from old.id
       or new.response_mode is distinct from old.response_mode
       or new.input is distinct from old.input
       or new.input_digest is distinct from old.input_digest
       or new.requested_at is distinct from old.requested_at
       or new.aggregate_version <> old.aggregate_version + 1
       or new.updated_at < old.updated_at then
        raise exception 'Application invocation update is stale or changes immutable request state';
    end if;

    valid_transition :=
        old.status = 'requested'
            and old.workflow_run_id is null
            and (
                new.status = 'running' and new.workflow_run_id is not null
                or new.status in ('cancelling', 'cancelled') and new.workflow_run_id is null
            )
        or old.status = 'running'
            and old.workflow_run_id is not null
            and new.workflow_run_id = old.workflow_run_id
            and new.status in ('cancelling', 'succeeded', 'failed', 'cancelled')
        or old.status = 'cancelling'
            and new.workflow_run_id is not distinct from old.workflow_run_id
            and new.status in ('succeeded', 'failed', 'cancelled');
    if not valid_transition then
        raise exception 'Application invocation transition is invalid';
    end if;
    return new;
end
$$;

create function validate_application_session_head()
returns trigger
language plpgsql
as $$
declare
    stored_message_count bigint;
begin
    perform 1
      from application_releases release
      join application_end_users end_user
        on end_user.organization_id = new.organization_id
       and end_user.project_id = new.project_id
       and end_user.application_id = new.application_id
       and end_user.id = new.end_user_id
     where release.organization_id = new.organization_id
       and release.project_id = new.project_id
       and release.application_id = new.application_id
       and release.id = new.application_release_id
       and release.release_number = new.application_release_number
       and release.contract_digest = new.application_release_digest
       and new.created_at >= release.created_at
       and new.created_at >= end_user.created_at
       and new.interaction_mode = case
           when release.experience in ('text_generator', 'workflow') then 'invocation'
           else 'conversation'
       end;
    if not found then
        raise exception 'Application session does not match its exact release and end user';
    end if;

    perform 1
      from application_conversation_variable_revisions revision
     where revision.organization_id = new.organization_id
       and revision.project_id = new.project_id
       and revision.application_id = new.application_id
       and revision.application_release_id = new.application_release_id
       and revision.application_release_digest = new.application_release_digest
       and revision.session_id = new.id
       and revision.id = new.current_variable_revision_id
       and revision.revision_number = new.current_variable_revision_number
       and revision.values_digest = new.current_variable_digest
       and revision.created_at <= new.updated_at;
    if not found then
        raise exception 'Application session variable head is missing or drifted';
    end if;

    select count(*) into stored_message_count
      from application_messages message
     where message.organization_id = new.organization_id
       and message.project_id = new.project_id
       and message.application_id = new.application_id
       and message.session_id = new.id;
    if stored_message_count <> new.last_message_sequence
       or new.last_message_sequence > 0 and not exists (
           select 1
             from application_messages message
            where message.organization_id = new.organization_id
              and message.project_id = new.project_id
              and message.application_id = new.application_id
              and message.session_id = new.id
              and message.sequence = new.last_message_sequence
       ) then
        raise exception 'Application session message head is missing or non-contiguous';
    end if;
    return null;
end
$$;

create function validate_application_message_record()
returns trigger
language plpgsql
as $$
begin
    perform 1
      from application_invocations invocation
     where invocation.organization_id = new.organization_id
       and invocation.project_id = new.project_id
       and invocation.application_id = new.application_id
       and invocation.application_release_id = new.application_release_id
       and invocation.application_release_digest = new.application_release_digest
       and invocation.session_id = new.session_id
       and invocation.id = new.invocation_id
       and new.created_at >= invocation.requested_at
       and (
           new.kind = 'input'
               and new.content = invocation.input
               and new.content_digest = invocation.input_digest
           or new.kind in ('answer', 'final_output')
               and new.workflow_run_id = invocation.workflow_run_id
               and (
                   new.kind <> 'final_output'
                   or invocation.status not in ('failed', 'cancelled')
               )
       );
    if not found then
        raise exception 'Application message does not match its exact invocation';
    end if;
    if exists (
        select 1
          from application_messages final_output
         where final_output.organization_id = new.organization_id
           and final_output.application_id = new.application_id
           and final_output.invocation_id = new.invocation_id
           and final_output.kind = 'final_output'
           and final_output.id <> new.id
           and final_output.sequence < new.sequence
    ) then
        raise exception 'Application invocation already has a final output';
    end if;
    return null;
end
$$;

create function validate_application_variable_revision()
returns trigger
language plpgsql
as $$
begin
    if new.revision_number = 1 then
        if new.created_at <> (
            select session.created_at
              from application_sessions session
             where session.organization_id = new.organization_id
               and session.project_id = new.project_id
               and session.application_id = new.application_id
               and session.id = new.session_id
        ) then
            raise exception 'Initial Application conversation variables must open with the session';
        end if;
        return null;
    end if;

    perform 1
      from application_conversation_variable_revisions parent
      join application_invocations invocation
        on invocation.organization_id = new.organization_id
       and invocation.project_id = new.project_id
       and invocation.application_id = new.application_id
       and invocation.session_id = new.session_id
       and invocation.workflow_run_id = new.workflow_run_id
     where parent.organization_id = new.organization_id
       and parent.project_id = new.project_id
       and parent.application_id = new.application_id
       and parent.session_id = new.session_id
       and parent.id = new.parent_revision_id
       and parent.revision_number + 1 = new.revision_number
       and parent.values_digest = new.parent_digest
       and new.created_at >= parent.created_at;
    if not found then
        raise exception 'Application conversation variable lineage or WorkflowRun owner drifted';
    end if;
    return null;
end
$$;

create function validate_application_effect_claim()
returns trigger
language plpgsql
as $$
begin
    perform 1
      from application_invocations invocation
     where invocation.organization_id = new.organization_id
       and invocation.project_id = new.project_id
       and invocation.application_id = new.application_id
       and invocation.session_id = new.session_id
       and invocation.workflow_run_id = new.workflow_run_id;
    if not found then
        raise exception 'Application Workflow effect has no exact invocation owner';
    end if;

    if new.semantic_kind in ('message_answer', 'message_final_output') then
        perform 1
          from application_messages message
         where message.organization_id = new.organization_id
           and message.project_id = new.project_id
           and message.application_id = new.application_id
           and message.session_id = new.session_id
           and message.id = new.semantic_id
           and message.workflow_run_id = new.workflow_run_id
           and message.workflow_step_id = new.workflow_step_id
           and message.workflow_attempt = new.workflow_attempt
           and message.workflow_effect_ordinal = new.workflow_effect_ordinal
           and message.kind = case new.semantic_kind
               when 'message_answer' then 'answer'
               else 'final_output'
           end;
    else
        perform 1
          from application_conversation_variable_revisions revision
         where revision.organization_id = new.organization_id
           and revision.project_id = new.project_id
           and revision.application_id = new.application_id
           and revision.session_id = new.session_id
           and revision.id = new.semantic_id
           and revision.workflow_run_id = new.workflow_run_id
           and revision.workflow_step_id = new.workflow_step_id
           and revision.workflow_attempt = new.workflow_attempt
           and revision.workflow_effect_ordinal = new.workflow_effect_ordinal;
    end if;
    if not found then
        raise exception 'Application Workflow effect claim does not match its semantic write';
    end if;
    return null;
end
$$;

create function validate_application_message_effect_claim()
returns trigger
language plpgsql
as $$
declare
    expected_kind text;
begin
    if new.kind = 'input' then
        return null;
    end if;
    expected_kind := case
        when new.kind = 'answer' then 'message_answer'
        else 'message_final_output'
    end;
    perform 1
      from application_workflow_effect_claims claim
     where claim.organization_id = new.organization_id
       and claim.project_id = new.project_id
       and claim.application_id = new.application_id
       and claim.session_id = new.session_id
       and claim.workflow_run_id = new.workflow_run_id
       and claim.workflow_step_id = new.workflow_step_id
       and claim.workflow_attempt = new.workflow_attempt
       and claim.workflow_effect_ordinal = new.workflow_effect_ordinal
       and claim.semantic_kind = expected_kind
       and claim.semantic_id = new.id;
    if not found then
        raise exception 'Application semantic write is missing its exact Workflow effect claim';
    end if;
    return null;
end
$$;

create function validate_application_variable_effect_claim()
returns trigger
language plpgsql
as $$
begin
    if new.revision_number = 1 then
        return null;
    end if;
    perform 1
      from application_workflow_effect_claims claim
     where claim.organization_id = new.organization_id
       and claim.project_id = new.project_id
       and claim.application_id = new.application_id
       and claim.session_id = new.session_id
       and claim.workflow_run_id = new.workflow_run_id
       and claim.workflow_step_id = new.workflow_step_id
       and claim.workflow_attempt = new.workflow_attempt
       and claim.workflow_effect_ordinal = new.workflow_effect_ordinal
       and claim.semantic_kind = 'conversation_variables'
       and claim.semantic_id = new.id;
    if not found then
        raise exception 'Application variable write is missing its exact Workflow effect claim';
    end if;
    return null;
end
$$;

create trigger application_sessions_update_guard
before update on application_sessions
for each row execute function validate_application_session_update();

create trigger application_invocations_update_guard
before update on application_invocations
for each row execute function validate_application_invocation_update();

create trigger application_end_users_immutable
before update or delete on application_end_users
for each row execute function reject_application_session_child_mutation();

create trigger application_sessions_delete_rejected
before delete on application_sessions
for each row execute function reject_application_session_child_mutation();

create trigger application_invocations_delete_rejected
before delete on application_invocations
for each row execute function reject_application_session_child_mutation();

create trigger application_messages_immutable
before update or delete on application_messages
for each row execute function reject_application_session_child_mutation();

create trigger application_variable_revisions_immutable
before update or delete on application_conversation_variable_revisions
for each row execute function reject_application_session_child_mutation();

create trigger application_effect_claims_immutable
before update or delete on application_workflow_effect_claims
for each row execute function reject_application_session_child_mutation();

create constraint trigger application_sessions_validate_head
after insert or update on application_sessions
deferrable initially deferred
for each row execute function validate_application_session_head();

create constraint trigger application_messages_validate_record
after insert on application_messages
deferrable initially deferred
for each row execute function validate_application_message_record();

create constraint trigger application_variables_validate_lineage
after insert on application_conversation_variable_revisions
deferrable initially deferred
for each row execute function validate_application_variable_revision();

create constraint trigger application_effect_claims_validate_target
after insert on application_workflow_effect_claims
deferrable initially deferred
for each row execute function validate_application_effect_claim();

create constraint trigger application_messages_validate_effect_claim
after insert on application_messages
deferrable initially deferred
for each row execute function validate_application_message_effect_claim();

create constraint trigger application_variables_validate_effect_claim
after insert on application_conversation_variable_revisions
deferrable initially deferred
for each row execute function validate_application_variable_effect_claim();

comment on table application_sessions is
    'Applications-owned exact-release session head; A3S Flow retains run history, attempts, scheduling, replay, and cancellation';

comment on table application_invocations is
    'Applications request and delivery correlation referencing one ordinary WorkflowRun without copying execution state';

comment on table application_workflow_effect_claims is
    'Cross-kind exactly-once fence for Applications semantic projections of exact WorkflowRun effects; not a Flow event log';
