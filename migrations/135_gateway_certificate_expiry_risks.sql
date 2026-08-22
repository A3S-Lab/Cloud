create table gateway_certificate_expiry_risks (
    organization_id uuid not null references organizations(id),
    route_id uuid not null references routes(id),
    node_id uuid not null,
    state text not null check (state in ('at_risk', 'clear')),
    active_certificate_id uuid not null references gateway_certificates(id),
    active_certificate_expires_at timestamptz not null,
    gateway_revision bigint not null check (gateway_revision > 0),
    generation bigint not null check (generation > 0),
    previous_at_risk_certificate_id uuid references gateway_certificates(id),
    previous_at_risk_certificate_expires_at timestamptz,
    created_at timestamptz not null,
    updated_at timestamptz not null,
    primary key (route_id, node_id),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (node_id, gateway_revision)
        references gateway_publications (node_id, revision),
    check (updated_at >= created_at),
    check (
        state = 'at_risk'
            and active_certificate_expires_at <= updated_at + interval '24 hours'
            and previous_at_risk_certificate_id is null
            and previous_at_risk_certificate_expires_at is null
        or state = 'clear'
            and active_certificate_expires_at > updated_at + interval '24 hours'
            and previous_at_risk_certificate_id is not null
            and previous_at_risk_certificate_id <> active_certificate_id
            and previous_at_risk_certificate_expires_at is not null
    )
);

comment on table gateway_certificate_expiry_risks is
    'Edge-owned monotonic Route-plus-node certificate expiry-risk transition projection; not Notification incident state, a configurable threshold, poller, timer, scheduler, queue, or second event rail';

comment on column gateway_certificate_expiry_risks.generation is
    'Strictly increasing owner-fact generation advanced only by the Edge certificate lifecycle under optimistic concurrency';
