-- AUT0.3-C8: allow a fenced scheduler owner to release a lease without
-- advancing the durable cursor when no work is due or admission must retry.
-- The existing trigger remains the single state-transition authority; this
-- migration only distinguishes a cursor commit from an equal-cursor release.

create or replace function enforce_automation_schedule_state_transition()
returns trigger
language plpgsql
as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'Automation schedule state cannot be deleted';
    end if;

    if old.organization_id <> new.organization_id
       or old.project_id <> new.project_id
       or old.environment_id <> new.environment_id
       or old.automation_id <> new.automation_id
       or old.revision_id <> new.revision_id
       or old.revision_digest <> new.revision_digest
       or old.created_at <> new.created_at then
        raise exception 'Automation schedule state identity and revision binding are immutable';
    end if;

    if new.lease_generation < old.lease_generation then
        raise exception 'Automation schedule lease generation cannot move backwards';
    end if;

    if old.lease_owner_id is null and new.lease_owner_id is not null then
        if new.lease_generation <> old.lease_generation + 1
           or new.cursor_at <> old.cursor_at
           or new.reserved_at < old.updated_at then
            raise exception 'Automation schedule lease acquisition is not fenced';
        end if;
    elsif old.lease_owner_id is not null and new.lease_owner_id is null then
        if new.lease_generation <> old.lease_generation
           or new.cursor_at < old.cursor_at
           or new.updated_at < old.reserved_at then
            raise exception 'Automation schedule lease release or commit is not fenced';
        end if;
    elsif old.lease_owner_id is not null and new.lease_owner_id is not null then
        if new.lease_generation <> old.lease_generation + 1
           or new.lease_id = old.lease_id
           or new.reserved_at < old.lease_expires_at
           or new.cursor_at <> old.cursor_at then
            raise exception 'Automation schedule lease takeover is not fenced';
        end if;
    else
        if new.cursor_at <> old.cursor_at
           or new.lease_generation <> old.lease_generation then
            raise exception 'Automation schedule state transition is invalid';
        end if;
    end if;
    return new;
end
$$;
