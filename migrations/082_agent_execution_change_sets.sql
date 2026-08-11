create table agent_execution_change_sets (
    organization_id uuid not null,
    execution_id uuid not null,
    batch_id uuid not null,
    node_id uuid not null,
    change_set jsonb not null,
    recorded_at timestamptz not null,
    primary key (organization_id, execution_id),
    unique (organization_id, batch_id),
    foreign key (organization_id, execution_id)
        references agent_executions (organization_id, id),
    check (jsonb_typeof(change_set) = 'object'),
    check (change_set ->> 'schema' = 'a3s.code.agent-change-set.v1'),
    check (change_set ->> 'format' = 'git_unified_diff_v1'),
    check (change_set ->> 'encoding' = 'base64'),
    check (change_set ->> 'state' in ('completed', 'failed', 'cancelled')),
    check (octet_length(change_set::text) between 2 and 5723477)
);

create function reject_agent_execution_change_set_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Agent execution change set is immutable';
end
$$;

create trigger agent_execution_change_sets_immutable
before update or delete on agent_execution_change_sets
for each row execute function reject_agent_execution_change_set_mutation();

comment on table agent_execution_change_sets is
    'Immutable Git-compatible workspace result captured from one exact terminal A3S Code run';
