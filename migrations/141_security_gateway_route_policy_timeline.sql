create index outbox_events_security_gateway_route_policy_timeline_idx
    on outbox_events (
        organization_id,
        aggregate_id,
        occurred_at desc,
        event_id desc
    )
    where event_key in (
        'edge.mcp-route-policy.created',
        'edge.mcp-route-policy.revised'
    );

create index audit_records_security_gateway_route_policy_correlation_idx
    on audit_records (
        organization_id,
        aggregate_id,
        action,
        occurred_at,
        request_id,
        audit_id
    )
    where action in (
        'edge.mcp-route-policy.created',
        'edge.mcp-route-policy.revised'
    );

comment on index outbox_events_security_gateway_route_policy_timeline_idx is
    'C0.3-S1a read-only owner-fact timeline index; Outbox remains the fact authority';

comment on index audit_records_security_gateway_route_policy_correlation_idx is
    'C0.3-S1a typed audit-metadata correlation only; audit details remain private';
