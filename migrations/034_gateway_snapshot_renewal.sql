alter table gateway_certificate_convergences
    drop constraint gateway_certificate_convergences_reason_check,
    add constraint gateway_certificate_convergences_reason_check
        check (
            reason in (
                'renewal',
                'snapshot_renewal',
                'domain_revocation',
                'certificate_revocation',
                'projection_repair'
            )
        );

alter table gateway_certificate_convergences
    drop constraint gateway_certificate_convergences_check1,
    add constraint gateway_certificate_convergences_certificate_transition_check
        check (
            reason = 'snapshot_renewal'
                and jsonb_array_length(retained_routes) > 0
                and jsonb_array_length(rejected_routes) = 0
                and replacement_certificate_id is null
            or reason <> 'snapshot_renewal'
                and (
                    jsonb_array_length(retained_routes) = 0
                        and replacement_certificate_id is null
                    or jsonb_array_length(retained_routes) > 0
                        and replacement_certificate_id is not null
                )
        );
