alter table workflow_step_projections
    drop constraint workflow_step_projections_kind_check;

alter table workflow_step_projections
    add constraint workflow_step_projections_kind_check check (
        kind in ('input', 'transform', 'branch', 'human_decision', 'output')
    );

create table form_submissions (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    workflow_run_id uuid not null,
    human_task_id uuid not null,
    form_id uuid not null,
    form_release_id uuid not null,
    flow_run_id text not null check (octet_length(flow_run_id) between 1 and 512),
    flow_hook_id text not null check (octet_length(flow_hook_id) between 1 and 512),
    step_id text not null check (step_id ~ '^[A-Za-z][A-Za-z0-9_-]{0,127}$'),
    step_attempt bigint not null check (step_attempt > 0),
    principal_id uuid not null references identity_principals(id),
    authorization_decision_id text not null check (
        octet_length(authorization_decision_id) between 1 and 512
    ),
    authorization_decision_digest text not null check (
        authorization_decision_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    outcome text not null check (outcome in ('submit', 'approve', 'reject')),
    interaction_request_digest text not null check (
        interaction_request_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    interaction_submission_id text not null check (
        octet_length(interaction_submission_id) between 1 and 512
    ),
    idempotency_key text not null check (octet_length(idempotency_key) between 1 and 512),
    candidate_value_digest text not null check (
        candidate_value_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    output_digest text not null check (output_digest ~ '^sha256:[0-9a-f]{64}$'),
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version = 1),
    record_json text not null check (octet_length(record_json) between 2 and 2097152),
    submitted_at timestamptz not null,
    accepted_at timestamptz not null,
    primary key (organization_id, id),
    unique (organization_id, project_id, workflow_run_id, human_task_id, id),
    unique (organization_id, project_id, workflow_run_id, human_task_id, id, digest),
    constraint form_submissions_interaction_identity_unique
        unique (organization_id, human_task_id, interaction_submission_id),
    constraint form_submissions_idempotency_unique
        unique (organization_id, human_task_id, idempotency_key),
    foreign key (organization_id, project_id, workflow_run_id)
        references workflow_runs (organization_id, project_id, id),
    foreign key (organization_id, project_id, form_id, form_release_id)
        references form_releases (organization_id, project_id, form_id, id),
    check (id::text = interaction_submission_id),
    check (flow_run_id = workflow_run_id::text),
    check (accepted_at >= submitted_at)
);

create table human_tasks (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    workflow_run_id uuid not null,
    step_id text not null check (step_id ~ '^[A-Za-z][A-Za-z0-9_-]{0,127}$'),
    step_attempt bigint not null check (step_attempt > 0),
    form_id uuid not null,
    form_release_id uuid not null,
    assignment_policy_id text not null check (
        octet_length(assignment_policy_id) between 1 and 512
    ),
    assignment_policy_revision bigint not null check (assignment_policy_revision > 0),
    assignment_policy_digest text not null check (
        assignment_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    flow_run_id text not null check (octet_length(flow_run_id) between 1 and 512),
    flow_hook_id text not null check (octet_length(flow_hook_id) between 1 and 512),
    status text not null check (
        status in (
            'pending_activation',
            'ready',
            'claimed',
            'completed',
            'expired',
            'cancelled'
        )
    ),
    claimed_by uuid references identity_principals(id),
    decision_id uuid,
    aggregate_version bigint not null check (aggregate_version > 0),
    task_json text not null check (octet_length(task_json) between 2 and 2097152),
    interaction_spec_json text not null check (
        octet_length(interaction_spec_json) between 2 and 2097152
    ),
    interaction_request_json text check (
        interaction_request_json is null
        or octet_length(interaction_request_json) between 2 and 2097152
    ),
    interaction_request_digest text check (
        interaction_request_digest is null
        or interaction_request_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    hook_event_sequence bigint not null check (hook_event_sequence > 0),
    hook_event_id uuid not null,
    created_at timestamptz not null,
    updated_at timestamptz not null,
    due_at timestamptz,
    expires_at timestamptz,
    claimed_at timestamptz,
    terminal_at timestamptz,
    primary key (organization_id, id),
    unique (organization_id, project_id, workflow_run_id, id),
    constraint human_tasks_run_step_generation_unique
        unique (organization_id, workflow_run_id, step_id, step_attempt),
    constraint human_tasks_flow_hook_unique
        unique (organization_id, flow_run_id, flow_hook_id),
    constraint human_tasks_hook_event_unique
        unique (organization_id, workflow_run_id, hook_event_id),
    foreign key (organization_id, project_id, workflow_run_id)
        references workflow_runs (organization_id, project_id, id),
    foreign key (organization_id, project_id, form_id, form_release_id)
        references form_releases (organization_id, project_id, form_id, id),
    check (flow_run_id = workflow_run_id::text),
    check ((interaction_request_json is null) = (interaction_request_digest is null)),
    check (
        case status
            when 'pending_activation' then
                claimed_by is null and claimed_at is null
                and interaction_request_json is null
            when 'ready' then
                claimed_by is null and claimed_at is null
                and interaction_request_json is null
            when 'claimed' then
                claimed_by is not null and claimed_at is not null
                and interaction_request_json is not null
            when 'completed' then
                claimed_by is not null and claimed_at is not null
                and interaction_request_json is not null
            else
                (claimed_by is null) = (claimed_at is null)
                and (claimed_by is null) = (interaction_request_json is null)
        end
    ),
    check (
        (status in ('completed', 'expired', 'cancelled'))
        = (decision_id is not null and terminal_at is not null)
    ),
    check (updated_at >= created_at),
    check (due_at is null or due_at >= created_at),
    check (expires_at is null or expires_at >= created_at),
    check (due_at is null or expires_at is null or due_at <= expires_at),
    check (claimed_at is null or claimed_at between created_at and updated_at),
    check (terminal_at is null or terminal_at = updated_at)
);

alter table form_submissions
    add constraint form_submissions_human_task_fk
    foreign key (organization_id, project_id, workflow_run_id, human_task_id)
    references human_tasks (organization_id, project_id, workflow_run_id, id);

create table workflow_decisions (
    organization_id uuid not null,
    project_id uuid not null,
    id uuid not null,
    workflow_run_id uuid not null,
    human_task_id uuid not null,
    flow_run_id text not null check (octet_length(flow_run_id) between 1 and 512),
    flow_hook_id text not null check (octet_length(flow_hook_id) between 1 and 512),
    step_id text not null check (step_id ~ '^[A-Za-z][A-Za-z0-9_-]{0,127}$'),
    step_attempt bigint not null check (step_attempt > 0),
    task_version bigint not null check (task_version > 0),
    form_id uuid not null,
    form_release_id uuid not null,
    assignment_policy_id text not null check (
        octet_length(assignment_policy_id) between 1 and 512
    ),
    assignment_policy_revision bigint not null check (assignment_policy_revision > 0),
    assignment_policy_digest text not null check (
        assignment_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    outcome text not null check (
        outcome in ('submit', 'approve', 'reject', 'expire', 'cancel')
    ),
    form_submission_id uuid,
    form_submission_digest text check (
        form_submission_digest is null
        or form_submission_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    decided_by uuid not null references identity_principals(id),
    authorization_decision_id text not null check (
        octet_length(authorization_decision_id) between 1 and 512
    ),
    authorization_decision_digest text not null check (
        authorization_decision_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    output_digest text not null check (output_digest ~ '^sha256:[0-9a-f]{64}$'),
    digest text not null check (digest ~ '^sha256:[0-9a-f]{64}$'),
    record_json text not null check (octet_length(record_json) between 2 and 2097152),
    decided_at timestamptz not null,
    primary key (organization_id, id),
    constraint workflow_decisions_one_per_task
        unique (organization_id, human_task_id),
    unique (organization_id, project_id, workflow_run_id, human_task_id, id),
    foreign key (organization_id, project_id, workflow_run_id, human_task_id)
        references human_tasks (organization_id, project_id, workflow_run_id, id),
    foreign key (organization_id, project_id, form_id, form_release_id)
        references form_releases (organization_id, project_id, form_id, id),
    constraint workflow_decisions_submission_fk
        foreign key (
            organization_id,
            project_id,
            workflow_run_id,
            human_task_id,
            form_submission_id,
            form_submission_digest
        ) references form_submissions (
            organization_id,
            project_id,
            workflow_run_id,
            human_task_id,
            id,
            digest
        ),
    check (flow_run_id = workflow_run_id::text),
    check (
        (outcome in ('submit', 'approve', 'reject'))
        = (form_submission_id is not null and form_submission_digest is not null)
    )
);

alter table human_tasks
    add constraint human_tasks_decision_fk
    foreign key (
        organization_id,
        project_id,
        workflow_run_id,
        id,
        decision_id
    ) references workflow_decisions (
        organization_id,
        project_id,
        workflow_run_id,
        human_task_id,
        id
    )
    deferrable initially deferred;

create table workflow_human_task_inbox (
    organization_id uuid not null,
    workflow_run_id uuid not null,
    flow_sequence bigint not null check (flow_sequence > 0),
    event_id uuid not null,
    event_key text not null check (
        event_key in ('flow.hook.created', 'flow.hook.received', 'flow.hook.disposed')
    ),
    event_digest text not null check (event_digest ~ '^sha256:[0-9a-f]{64}$'),
    observed_at timestamptz not null,
    processed_at timestamptz not null,
    primary key (organization_id, workflow_run_id, flow_sequence),
    unique (organization_id, workflow_run_id, event_id),
    foreign key (organization_id, workflow_run_id)
        references workflow_runs (organization_id, id),
    check (processed_at >= observed_at)
);

create table workflow_resume_outbox (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_decision_id uuid not null,
    workflow_run_id uuid not null,
    human_task_id uuid not null,
    flow_run_id text not null check (octet_length(flow_run_id) between 1 and 512),
    flow_hook_id text not null check (octet_length(flow_hook_id) between 1 and 512),
    payload_json text not null check (octet_length(payload_json) between 2 and 2097152),
    payload_digest text not null check (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    state text not null check (state in ('pending', 'delivering', 'delivered', 'conflicted')),
    attempt_count integer not null check (attempt_count >= 0),
    available_at timestamptz not null,
    lease_owner uuid,
    lease_expires_at timestamptz,
    last_error text check (last_error is null or octet_length(last_error) between 1 and 16384),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    delivered_at timestamptz,
    primary key (organization_id, workflow_decision_id),
    unique (organization_id, project_id, workflow_run_id, human_task_id, workflow_decision_id),
    foreign key (
        organization_id,
        project_id,
        workflow_run_id,
        human_task_id,
        workflow_decision_id
    ) references workflow_decisions (
        organization_id,
        project_id,
        workflow_run_id,
        human_task_id,
        id
    ),
    check (flow_run_id = workflow_run_id::text),
    check (
        (state = 'delivering')
        = (lease_owner is not null and lease_expires_at is not null)
    ),
    check ((state = 'delivered') = (delivered_at is not null)),
    check (updated_at >= created_at),
    check (available_at >= created_at),
    check (delivered_at is null or delivered_at >= created_at)
);

create table workflow_resume_receipts (
    organization_id uuid not null,
    project_id uuid not null,
    workflow_decision_id uuid not null,
    workflow_run_id uuid not null,
    human_task_id uuid not null,
    flow_run_id text not null check (octet_length(flow_run_id) between 1 and 512),
    flow_hook_id text not null check (octet_length(flow_hook_id) between 1 and 512),
    payload_digest text not null check (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    hook_event_sequence bigint not null check (hook_event_sequence > 0),
    hook_event_id uuid not null,
    hook_received_at timestamptz not null,
    receipt_json text not null check (octet_length(receipt_json) between 2 and 65536),
    recorded_at timestamptz not null,
    primary key (organization_id, workflow_decision_id),
    unique (organization_id, workflow_run_id, hook_event_id),
    foreign key (
        organization_id,
        project_id,
        workflow_run_id,
        human_task_id,
        workflow_decision_id
    ) references workflow_resume_outbox (
        organization_id,
        project_id,
        workflow_run_id,
        human_task_id,
        workflow_decision_id
    ),
    check (flow_run_id = workflow_run_id::text),
    check (recorded_at >= hook_received_at)
);

create index human_tasks_project_status_idx
    on human_tasks (organization_id, project_id, status, created_at, id);

create index human_tasks_run_step_idx
    on human_tasks (organization_id, workflow_run_id, step_id, step_attempt);

create index workflow_resume_outbox_delivery_idx
    on workflow_resume_outbox (available_at, created_at, workflow_decision_id)
    where state in ('pending', 'delivering');

create index workflow_human_task_inbox_observed_idx
    on workflow_human_task_inbox (observed_at, workflow_run_id, flow_sequence);

create function reject_human_task_immutable_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Human task immutable record cannot be changed';
end
$$;

create function protect_human_task_authority()
returns trigger
language plpgsql
as $$
begin
    if old.organization_id is distinct from new.organization_id
        or old.project_id is distinct from new.project_id
        or old.id is distinct from new.id
        or old.workflow_run_id is distinct from new.workflow_run_id
        or old.step_id is distinct from new.step_id
        or old.step_attempt is distinct from new.step_attempt
        or old.form_id is distinct from new.form_id
        or old.form_release_id is distinct from new.form_release_id
        or old.assignment_policy_id is distinct from new.assignment_policy_id
        or old.assignment_policy_revision is distinct from new.assignment_policy_revision
        or old.assignment_policy_digest is distinct from new.assignment_policy_digest
        or old.flow_run_id is distinct from new.flow_run_id
        or old.flow_hook_id is distinct from new.flow_hook_id
        or old.interaction_spec_json is distinct from new.interaction_spec_json
        or old.hook_event_sequence is distinct from new.hook_event_sequence
        or old.hook_event_id is distinct from new.hook_event_id
        or old.created_at is distinct from new.created_at
        or old.due_at is distinct from new.due_at
        or old.expires_at is distinct from new.expires_at
    then
        raise exception 'HumanTask immutable authority cannot be changed';
    end if;
    return new;
end
$$;

create function protect_workflow_resume_outbox_authority()
returns trigger
language plpgsql
as $$
begin
    if old.organization_id is distinct from new.organization_id
        or old.project_id is distinct from new.project_id
        or old.workflow_decision_id is distinct from new.workflow_decision_id
        or old.workflow_run_id is distinct from new.workflow_run_id
        or old.human_task_id is distinct from new.human_task_id
        or old.flow_run_id is distinct from new.flow_run_id
        or old.flow_hook_id is distinct from new.flow_hook_id
        or old.payload_json is distinct from new.payload_json
        or old.payload_digest is distinct from new.payload_digest
        or old.created_at is distinct from new.created_at
    then
        raise exception 'Workflow resume Outbox immutable authority cannot be changed';
    end if;
    return new;
end
$$;

create trigger form_submissions_immutable
before update or delete on form_submissions
for each row execute function reject_human_task_immutable_mutation();

create trigger workflow_decisions_immutable
before update or delete on workflow_decisions
for each row execute function reject_human_task_immutable_mutation();

create trigger workflow_human_task_inbox_immutable
before update or delete on workflow_human_task_inbox
for each row execute function reject_human_task_immutable_mutation();

create trigger workflow_resume_receipts_immutable
before update or delete on workflow_resume_receipts
for each row execute function reject_human_task_immutable_mutation();

create trigger human_tasks_authority_immutable
before update on human_tasks
for each row execute function protect_human_task_authority();

create trigger human_tasks_delete_rejected
before delete on human_tasks
for each row execute function reject_human_task_immutable_mutation();

create trigger workflow_resume_outbox_authority_immutable
before update on workflow_resume_outbox
for each row execute function protect_workflow_resume_outbox_authority();

create trigger workflow_resume_outbox_delete_rejected
before delete on workflow_resume_outbox
for each row execute function reject_human_task_immutable_mutation();

comment on table form_submissions is
    'Forms-owned immutable accepted submissions bound to an exact HumanTask generation and authorization decision';

comment on table human_tasks is
    'Workflow-owned optimistic human work coordinated by one exact A3S Flow hook';

comment on table workflow_decisions is
    'Workflow-owned immutable terminal decisions for HumanTask aggregates';

comment on table workflow_human_task_inbox is
    'Immutable deduplication evidence for correlated A3S Flow hook events';

comment on table workflow_resume_outbox is
    'Durable exactly-bound A3S Flow hook resume delivery intent';

comment on table workflow_resume_receipts is
    'Immutable receipt derived only from the exact matching A3S Flow HookReceived event';
