create table node_protocol_session_heads (
    organization_id uuid not null,
    node_id uuid primary key,
    agent_instance_id uuid not null check (
        agent_instance_id <> '00000000-0000-0000-0000-000000000000'
    ),
    session_epoch uuid not null check (
        session_epoch <> '00000000-0000-0000-0000-000000000000'
    ),
    hello_sequence bigint not null check (
        hello_sequence between 1 and 9007199254740991
    ),
    session_id uuid not null unique check (
        session_id <> '00000000-0000-0000-0000-000000000000'
    ),
    generation bigint not null check (
        generation between 1 and 9007199254740991
    ),
    contracts_digest text not null check (
        contracts_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    selected_at timestamptz not null,
    expires_at timestamptz not null,
    hello jsonb not null,
    selection jsonb not null,
    unique (organization_id, node_id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    check (
        expires_at > selected_at
        and expires_at <= selected_at + interval '24 hours'
    ),
    check (
        jsonb_typeof(hello) = 'object'
        and hello ->> 'schema' = 'a3s.cloud.node-session-hello.v1'
        and hello ->> 'node_id' = node_id::text
        and hello ->> 'agent_instance_id' = agent_instance_id::text
        and hello ->> 'session_epoch' = session_epoch::text
        and (hello ->> 'hello_sequence')::bigint = hello_sequence
    ),
    check (
        jsonb_typeof(selection) = 'object'
        and selection ->> 'schema' =
            'a3s.cloud.node-session-selection.v1'
        and selection ->> 'node_id' = node_id::text
        and selection ->> 'agent_instance_id' = agent_instance_id::text
        and selection ->> 'session_epoch' = session_epoch::text
        and (selection ->> 'hello_sequence')::bigint = hello_sequence
        and selection ->> 'session_id' = session_id::text
        and (selection ->> 'generation')::bigint = generation
        and (selection ->> 'selected_at')::timestamptz = selected_at
        and (selection ->> 'expires_at')::timestamptz = expires_at
    )
);

comment on table node_protocol_session_heads is
    'Fleet-owned current node protocol selection; exact reconnect history is carried by the digest-bound selection chain';
