alter table node_gateway_acknowledgements
    add column management_protocol text,
    add column snapshot_request_schema text,
    add column snapshot_status_schema text,
    add column protocol_discovery text;

alter table node_gateway_acknowledgements
    add constraint node_gateway_acknowledgements_protocol_shape_check
        check (
            management_protocol is null
            and snapshot_request_schema is null
            and snapshot_status_schema is null
            and protocol_discovery is null
            or management_protocol = 'a3s.gateway.management-protocol.v1'
            and snapshot_request_schema = 'a3s.gateway.managed-snapshot.v1'
            and snapshot_status_schema = 'a3s.gateway.managed-snapshot-status.v1'
            and protocol_discovery in ('advertised', 'legacy_version_v1')
        );
