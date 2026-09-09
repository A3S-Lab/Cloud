-- I0.2c: request facts and rebuildable daily rollups projected from
-- validated a3s.gateway.usage-lifecycle.v1 payloads. Rollups never store
-- request bodies, credentials, commercial amounts, or secret material.

create table inference_usage_request_facts (
    organization_id uuid not null check (
        organization_id <> '00000000-0000-0000-0000-000000000000'
    ),
    request_id uuid not null check (
        request_id <> '00000000-0000-0000-0000-000000000000'
    ),
    gateway_id uuid not null check (
        gateway_id <> '00000000-0000-0000-0000-000000000000'
    ),
    environment_id uuid not null check (
        environment_id <> '00000000-0000-0000-0000-000000000000'
    ),
    credential_id uuid not null check (
        credential_id <> '00000000-0000-0000-0000-000000000000'
    ),
    credential_generation bigint not null check (credential_generation > 0),
    route_id uuid not null check (
        route_id <> '00000000-0000-0000-0000-000000000000'
    ),
    route_policy_revision bigint not null check (route_policy_revision > 0),
    endpoint text not null check (
        endpoint in ('models', 'chat-completions', 'completions', 'embeddings')
    ),
    model_alias text not null check (
        char_length(model_alias) between 1 and 256
    ),
    model_id uuid not null check (
        model_id <> '00000000-0000-0000-0000-000000000000'
    ),
    started_at timestamptz not null,
    terminated_at timestamptz,
    outcome text check (
        outcome is null
        or outcome in (
            'succeeded',
            'failed',
            'fallback',
            'cancelled',
            'disconnected'
        )
    ),
    http_status integer check (
        http_status is null or (http_status >= 100 and http_status <= 599)
    ),
    duration_ms bigint check (duration_ms is null or duration_ms >= 0),
    measurement_completeness text check (
        measurement_completeness is null
        or measurement_completeness in ('unknown', 'upstream_usage')
    ),
    total_tokens bigint check (total_tokens is null or total_tokens >= 0),
    attempt_count integer not null check (attempt_count >= 0),
    updated_at timestamptz not null,
    primary key (organization_id, request_id),
    check (
        (outcome is null) = (terminated_at is null)
    )
);

create index inference_usage_request_facts_org_started_idx
    on inference_usage_request_facts (organization_id, started_at);

create table inference_usage_daily_rollups (
    organization_id uuid not null check (
        organization_id <> '00000000-0000-0000-0000-000000000000'
    ),
    day date not null,
    environment_id uuid not null check (
        environment_id <> '00000000-0000-0000-0000-000000000000'
    ),
    model_id uuid not null check (
        model_id <> '00000000-0000-0000-0000-000000000000'
    ),
    endpoint text not null check (
        endpoint in ('models', 'chat-completions', 'completions', 'embeddings')
    ),
    request_count bigint not null check (request_count >= 0),
    succeeded_count bigint not null check (succeeded_count >= 0),
    failed_count bigint not null check (failed_count >= 0),
    fallback_count bigint not null check (fallback_count >= 0),
    cancelled_count bigint not null check (cancelled_count >= 0),
    disconnected_count bigint not null check (disconnected_count >= 0),
    unknown_measurement_count bigint not null check (unknown_measurement_count >= 0),
    upstream_usage_count bigint not null check (upstream_usage_count >= 0),
    total_tokens bigint not null check (total_tokens >= 0),
    updated_at timestamptz not null,
    primary key (
        organization_id,
        day,
        environment_id,
        model_id,
        endpoint
    )
);
