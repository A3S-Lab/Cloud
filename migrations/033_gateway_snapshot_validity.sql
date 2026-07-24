alter table gateway_publications
    add column snapshot_expires_at timestamptz;

update gateway_publications
set snapshot_expires_at = command_not_after;

alter table gateway_publications
    alter column snapshot_expires_at set not null,
    add constraint gateway_publications_snapshot_validity_check
        check (
            snapshot_expires_at >= command_not_after
            and snapshot_expires_at > command_issued_at
            and snapshot_expires_at <= command_issued_at + interval '24 hours'
        );

alter table gateway_route_cutovers
    add column snapshot_expires_at timestamptz;

update gateway_route_cutovers as cutover
set snapshot_expires_at = publication.snapshot_expires_at
from gateway_publications as publication
where publication.node_id = cutover.node_id
  and publication.revision = cutover.gateway_revision
  and publication.command_id = cutover.gateway_command_id;

alter table gateway_route_cutovers
    alter column snapshot_expires_at set not null,
    add constraint gateway_route_cutovers_snapshot_validity_check
        check (snapshot_expires_at > staged_at);

alter table node_gateway_acknowledgements
    add column gateway_id uuid,
    add column expires_at timestamptz,
    add column ready boolean;

update node_gateway_acknowledgements
set gateway_id = node_id,
    expires_at = acknowledged_at + interval '1 microsecond',
    ready = state = 'applied';

update node_gateway_acknowledgements as acknowledgement
set expires_at = greatest(
        publication.snapshot_expires_at,
        acknowledgement.acknowledged_at + interval '1 microsecond'
    )
from gateway_publications as publication
where publication.node_id = acknowledgement.node_id
  and publication.revision = acknowledgement.revision
  and publication.command_id = acknowledgement.command_id;

alter table node_gateway_acknowledgements
    alter column gateway_id set not null,
    alter column expires_at set not null,
    alter column ready set not null,
    add constraint node_gateway_acknowledgements_gateway_identity_check
        check (gateway_id = node_id),
    add constraint node_gateway_acknowledgements_readiness_check
        check (
            state = 'applied' and ready and message is null
            or state = 'rejected' and not ready and message is not null
        ),
    add constraint node_gateway_acknowledgements_validity_check
        check (state = 'rejected' or acknowledged_at < expires_at);
