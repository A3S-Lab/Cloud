alter table gateway_certificate_convergences
    drop constraint gateway_certificate_convergences_state_check,
    add constraint gateway_certificate_convergences_state_check
        check (state in ('pending', 'applied', 'rejected', 'unavailable'));

alter table gateway_certificate_convergences
    drop constraint gateway_certificate_convergences_check3,
    add constraint gateway_certificate_convergences_terminal_state_check
        check (
            state = 'pending'
                and failure is null
                and acknowledged_at is null
            or state = 'applied'
                and failure is null
                and acknowledged_at is not null
            or state in ('rejected', 'unavailable')
                and failure is not null
                and acknowledged_at is not null
        );
