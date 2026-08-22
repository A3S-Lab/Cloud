alter table notification_alert_policies
    drop constraint notification_alert_policies_source_check,
    add constraint notification_alert_policies_source_check
    check (source in (
        'edge.domain-claim-status.v1',
        'edge.gateway-certificate-renewal-status.v1',
        'workload.deployment-health.v1'
    )) not valid;

alter table notification_alert_policies
    validate constraint notification_alert_policies_source_check;

comment on column notification_alert_policies.source is
    'Compile-time closed typed owner-event source registry; extended only by reviewed schema migrations';
