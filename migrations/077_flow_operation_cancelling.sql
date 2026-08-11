alter table operation_projections
    drop constraint operation_projections_status_check,
    add constraint operation_projections_status_check check (
        status in (
            'queued',
            'running',
            'suspended',
            'cancelling',
            'succeeded',
            'failed',
            'cancelled'
        )
    );
