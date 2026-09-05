-- AUT0.2-C3: durable endpoint and delivery admission state.
-- The canonical revision ACL and bounded request capture are retained so a
-- restart can restore the exact contract without resolving a mutable latest
-- Automation. Secret material is intentionally absent; only the endpoint
-- contract's Secret identity/version reference is present in endpoint_json.

create table automation_webhook_endpoints (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    endpoint_id uuid not null,
    endpoint_key text not null
        check (char_length(endpoint_key) between 1 and 128)
        check (endpoint_key ~ '^[a-z0-9_-]+$'),
    revision_id uuid not null,
    revision_digest text not null
        check (revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    revision_acl text not null
        check (octet_length(revision_acl) between 1 and 131072),
    endpoint_json jsonb not null
        check (jsonb_typeof(endpoint_json) = 'object'),
    generation bigint not null check (generation > 0),
    state text not null check (state in ('active', 'disabled', 'revoked')),
    created_at timestamptz not null,
    state_changed_at timestamptz,
    primary key (organization_id, endpoint_id),
    unique (endpoint_id),
    unique (organization_id, project_id, environment_id, endpoint_id),
    unique (organization_id, project_id, environment_id, endpoint_key),
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (state = 'active' and state_changed_at is null
        or state <> 'active' and state_changed_at is not null
        and state_changed_at >= created_at),
    check ((endpoint_json ->> 'endpointId')::uuid = endpoint_id),
    check ((endpoint_json ->> 'endpointKey') = endpoint_key),
    check ((endpoint_json ->> 'revisionId')::uuid = revision_id),
    check ((endpoint_json ->> 'revisionDigest') = revision_digest),
    check ((endpoint_json ->> 'generation')::bigint = generation),
    check ((endpoint_json ->> 'state') = state),
    check ((endpoint_json ->> 'createdAt')::timestamptz = created_at),
    check (
        (endpoint_json ->> 'stateChangedAt') is null
        and state_changed_at is null
        or (endpoint_json ->> 'stateChangedAt')::timestamptz = state_changed_at
    )
);

create index automation_webhook_endpoints_scope_idx
    on automation_webhook_endpoints (
        organization_id,
        project_id,
        environment_id,
        state,
        endpoint_key,
        endpoint_id
    );

create table automation_webhook_deliveries (
    organization_id uuid not null,
    endpoint_id uuid not null,
    delivery_id uuid not null,
    request_json jsonb not null
        check (jsonb_typeof(request_json) = 'object'),
    receipt_json jsonb not null
        check (jsonb_typeof(receipt_json) = 'object'),
    invocation_json jsonb,
    body_digest text not null
        check (body_digest ~ '^sha256:[0-9a-f]{64}$'),
    decision text not null check (decision in ('admitted', 'rejected')),
    first_received_at timestamptz not null,
    recorded_at timestamptz not null,
    primary key (organization_id, endpoint_id, delivery_id),
    unique (organization_id, endpoint_id, delivery_id, body_digest),
    foreign key (organization_id, endpoint_id)
        references automation_webhook_endpoints (organization_id, endpoint_id),
    check ((request_json ->> 'endpointId')::uuid = endpoint_id),
    check ((request_json ->> 'deliveryId')::uuid = delivery_id),
    check ((request_json ->> 'bodyDigest') = body_digest),
    check ((receipt_json ->> 'deliveryId')::uuid = delivery_id),
    check ((receipt_json ->> 'endpointId')::uuid = endpoint_id),
    check ((receipt_json ->> 'bodyDigest') = body_digest),
    check ((request_json ->> 'receivedAt')::timestamptz = first_received_at),
    check ((receipt_json ->> 'firstReceivedAt')::timestamptz = first_received_at),
    check ((receipt_json ->> 'recordedAt')::timestamptz = recorded_at),
    check ((receipt_json ->> 'decision') = decision),
    check (recorded_at >= first_received_at),
    check ((decision = 'admitted' and invocation_json is not null)
        or (decision = 'rejected' and invocation_json is null))
);

create index automation_webhook_deliveries_recovery_idx
    on automation_webhook_deliveries (
        organization_id,
        endpoint_id,
        recorded_at,
        delivery_id
    );

create table automation_webhook_delivery_receipts (
    receipt_id uuid primary key,
    organization_id uuid not null,
    endpoint_id uuid not null,
    delivery_id uuid not null,
    decision text not null check (decision in ('admitted', 'replayed', 'rejected')),
    receipt_json jsonb not null
        check (jsonb_typeof(receipt_json) = 'object'),
    recorded_at timestamptz not null,
    foreign key (organization_id, endpoint_id)
        references automation_webhook_endpoints (organization_id, endpoint_id),
    foreign key (organization_id, endpoint_id, delivery_id)
        references automation_webhook_deliveries (organization_id, endpoint_id, delivery_id),
    check ((receipt_json ->> 'receiptId')::uuid = receipt_id),
    check ((receipt_json ->> 'endpointId')::uuid = endpoint_id),
    check ((receipt_json ->> 'deliveryId')::uuid = delivery_id),
    check ((receipt_json ->> 'recordedAt')::timestamptz = recorded_at),
    check ((receipt_json ->> 'decision') = decision)
);

create index automation_webhook_delivery_receipts_delivery_idx
    on automation_webhook_delivery_receipts (
        organization_id,
        endpoint_id,
        delivery_id,
        recorded_at,
        receipt_id
    );

create function validate_automation_webhook_endpoint_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
         raise exception 'Automation webhook endpoint identity and revision are immutable';
    end if;

    if new.organization_id is distinct from old.organization_id
       or new.project_id is distinct from old.project_id
       or new.environment_id is distinct from old.environment_id
       or new.endpoint_id is distinct from old.endpoint_id
       or new.endpoint_key is distinct from old.endpoint_key
       or new.revision_id is distinct from old.revision_id
       or new.revision_digest is distinct from old.revision_digest
       or new.revision_acl is distinct from old.revision_acl
       or (new.endpoint_json - 'generation' - 'state' - 'stateChangedAt')
          is distinct from
          (old.endpoint_json - 'generation' - 'state' - 'stateChangedAt')
       or new.created_at is distinct from old.created_at then
        raise exception 'Automation webhook endpoint identity and revision are immutable';
    end if;

    if new.generation <> old.generation + 1
       or new.endpoint_json ->> 'generation' <> new.generation::text
       or new.endpoint_json ->> 'state' <> new.state then
        raise exception 'Automation webhook endpoint lifecycle generation is not sequential';
    end if;

    if not (
        old.state = 'active' and new.state in ('disabled', 'revoked')
        or old.state = 'disabled' and new.state in ('active', 'revoked')
    ) then
        raise exception 'Automation webhook endpoint lifecycle transition is invalid';
    end if;
    return new;
end
$$;

create trigger automation_webhook_endpoints_validate_transition
before update or delete on automation_webhook_endpoints
for each row execute function validate_automation_webhook_endpoint_transition();

create function reject_automation_webhook_delivery_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Automation webhook deliveries and receipts are immutable';
end
$$;

create trigger automation_webhook_deliveries_immutable
before update or delete on automation_webhook_deliveries
for each row execute function reject_automation_webhook_delivery_mutation();

create trigger automation_webhook_delivery_receipts_immutable
before update or delete on automation_webhook_delivery_receipts
for each row execute function reject_automation_webhook_delivery_mutation();

comment on table automation_webhook_endpoints is
    'Automations-owned exact-revision webhook endpoints; no Secret plaintext or mutable latest selector';

comment on table automation_webhook_deliveries is
    'One immutable first delivery capture and admission decision per endpoint/delivery identity';

comment on table automation_webhook_delivery_receipts is
    'Immutable admitted, replayed, or rejected receipt projections for webhook recovery';
