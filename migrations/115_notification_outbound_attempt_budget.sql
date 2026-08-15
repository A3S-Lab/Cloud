alter table notification_outbound_deliveries
    drop constraint notification_outbound_deliveries_terminal_outcome_check;

alter table notification_outbound_deliveries
    add constraint notification_outbound_deliveries_terminal_outcome_check
    check (
        terminal_outcome in (
            'delivered',
            'rejected',
            'indeterminate',
            'exhausted'
        )
    );

create or replace function validate_notification_outbound_terminal_receipt()
returns trigger
language plpgsql
as $$
declare
    attempt_state text;
    attempt_deadline timestamptz;
    evidence_outcome text;
    evidence_completed_at timestamptz;
begin
    if new.terminal_outcome is null then
        return null;
    end if;
    select state, outcome_deadline_at
      into attempt_state, attempt_deadline
      from connector_execution_attempts
     where organization_id = new.organization_id
       and project_id = new.connector_project_id
       and environment_id = new.connector_environment_id
       and profile_id = new.connector_profile_id
       and revision_id = new.connector_revision_id
       and attempt_id = new.terminal_attempt_id;
    select outcome, completed_at
      into evidence_outcome, evidence_completed_at
      from connector_execution_evidence
     where organization_id = new.organization_id
       and project_id = new.connector_project_id
       and environment_id = new.connector_environment_id
       and profile_id = new.connector_profile_id
       and revision_id = new.connector_revision_id
       and attempt_id = new.terminal_attempt_id;

    if new.terminal_outcome = 'delivered'
       and (
           attempt_state is distinct from 'terminal'
           or evidence_outcome is distinct from 'accepted'
           or new.terminal_at is distinct from evidence_completed_at
       )
       or new.terminal_outcome = 'rejected'
       and (
           attempt_state is distinct from 'terminal'
           or evidence_outcome is distinct from 'rejected'
           or new.terminal_at is distinct from evidence_completed_at
       )
       or new.terminal_outcome = 'indeterminate'
       and (
           attempt_state is distinct from 'dispatching'
           or evidence_outcome is not null
           or attempt_deadline is distinct from new.terminal_at
       )
       or new.terminal_outcome = 'exhausted'
       and (
           attempt_state is distinct from 'terminal'
           or evidence_outcome is distinct from 'retryable'
           or new.terminal_at is distinct from evidence_completed_at
           or new.terminal_generation is distinct from 8
       ) then
        raise exception 'Outbound notification terminal receipt does not match its exact C6 attempt and delivery budget';
    end if;
    return null;
end
$$;

comment on column notification_outbound_deliveries.terminal_outcome is
    'Monotonic logical result: delivered/rejected reference matching C6 evidence, indeterminate references an exact dispatch deadline, and exhausted references retryable evidence at the fixed eight-attempt provider budget';
