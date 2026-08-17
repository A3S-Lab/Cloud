alter table executions
    add column target_node_id uuid,
    add column task_policy jsonb,
    add constraint executions_bound_task_pair_check check (coalesce((
        (target_node_id is null and task_policy is null)
        or
        (
            target_node_id is not null
            and task_policy is not null
            and jsonb_typeof(task_policy) = 'object'
            and task_policy ?& array[
                'authority',
                'mounts',
                'secrets',
                'semanticsProfileDigest'
            ]
            and task_policy - array[
                'authority',
                'mounts',
                'secrets',
                'semanticsProfileDigest'
            ] = '{}'::jsonb
            and case
                when jsonb_typeof(task_policy -> 'authority') = 'object'
                    and jsonb_typeof(task_policy -> 'mounts') = 'array'
                    and jsonb_typeof(task_policy -> 'secrets') = 'array'
                then
                    (task_policy -> 'authority') ?& array['kind', 'subjectId', 'digest']
                    and (task_policy -> 'authority') - array['kind', 'subjectId', 'digest']
                        = '{}'::jsonb
                    and task_policy #>> '{authority,kind}'
                        ~ '^[a-z][a-z0-9._-]{0,95}$'
                    and task_policy #>> '{authority,subjectId}'
                        ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
                    and task_policy #>> '{authority,digest}'
                        ~ '^sha256:[0-9a-f]{64}$'
                    and task_policy ->> 'semanticsProfileDigest'
                        ~ '^sha256:[0-9a-f]{64}$'
                    and jsonb_array_length(task_policy -> 'mounts') between 1 and 128
                    and jsonb_array_length(task_policy -> 'secrets') between 1 and 128
                else false
            end
        )
    ), false)),
    add constraint executions_bound_task_node_fk foreign key (
        organization_id,
        target_node_id
    ) references nodes (organization_id, id);

comment on column executions.target_node_id is
    'Immutable exact-node fence for an internally composed finite Runtime Task; ordinary public Executions leave it null';

comment on column executions.task_policy is
    'Immutable internal Runtime Task inputs: read-only shared artifacts, opaque exact Secret references, outbound network, and owner digest; not product configuration or another task lifecycle';
