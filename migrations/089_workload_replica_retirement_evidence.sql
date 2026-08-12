alter table workload_replicas
    add column retirement_command_id uuid,
    add column runtime_fenced_at timestamptz;

alter table workload_replicas
    add constraint workload_replicas_retirement_evidence_check check (
        (
            lifecycle = 'desired'
            and retirement_command_id is null
            and runtime_fenced_at is null
        )
        or (
            lifecycle = 'retiring'
            and (
                runtime_fenced_at is null
                or retirement_command_id is not null
            )
        )
        or (
            lifecycle = 'retired'
            and (
                (retirement_command_id is null and runtime_fenced_at is null)
                or (retirement_command_id is not null and runtime_fenced_at is not null)
            )
        )
    );

alter table workload_replicas
    add constraint workload_replicas_runtime_fence_time_check check (
        runtime_fenced_at is null
        or (
            runtime_fenced_at >= created_at
            and runtime_fenced_at <= updated_at
        )
    );
