create table recipient_contact_verification_deliveries (
    verification_id uuid primary key references recipient_contact_verifications(id),
    state text not null check (
        state in ('reserved', 'dispatching', 'delivered', 'rejected', 'indeterminate', 'obsolete')
    ),
    fence_token uuid not null check (
        fence_token <> '00000000-0000-0000-0000-000000000000'
    ),
    reserved_at timestamptz not null,
    lease_expires_at timestamptz not null check (lease_expires_at > reserved_at),
    dispatch_started_at timestamptz,
    settled_at timestamptz,
    check (
        (state = 'reserved'
            and dispatch_started_at is null
            and settled_at is null)
        or (state = 'dispatching'
            and dispatch_started_at >= reserved_at
            and dispatch_started_at < lease_expires_at
            and settled_at is null)
        or (state in ('delivered', 'rejected', 'indeterminate')
            and dispatch_started_at >= reserved_at
            and dispatch_started_at < lease_expires_at
            and settled_at >= dispatch_started_at)
        or (state = 'obsolete'
            and dispatch_started_at is null
            and settled_at >= reserved_at)
    )
);

create function validate_recipient_contact_verification_delivery_insert()
returns trigger
language plpgsql
as $$
begin
    if new.state not in ('reserved', 'obsolete') then
        raise exception 'Recipient contact verification delivery must begin before dispatch';
    end if;
    return new;
end
$$;

create trigger recipient_contact_verification_deliveries_validate_insert
before insert on recipient_contact_verification_deliveries
for each row execute function validate_recipient_contact_verification_delivery_insert();

create function validate_recipient_contact_verification_delivery_transition()
returns trigger
language plpgsql
as $$
begin
    if new.verification_id <> old.verification_id then
        raise exception 'Recipient contact verification delivery identity is immutable';
    end if;
    if old.state in ('delivered', 'rejected', 'indeterminate', 'obsolete') then
        raise exception 'Recipient contact verification delivery terminal state is immutable';
    end if;
    if old.state = 'reserved' and new.state = 'reserved' then
        if new.reserved_at < old.lease_expires_at
           or new.fence_token = old.fence_token
           or new.dispatch_started_at is not null
           or new.settled_at is not null then
            raise exception 'Recipient contact verification delivery reservation renewal is invalid';
        end if;
    elsif old.state = 'reserved' and new.state in ('dispatching', 'obsolete') then
        if new.fence_token <> old.fence_token
           or new.reserved_at <> old.reserved_at
           or new.lease_expires_at <> old.lease_expires_at then
            raise exception 'Recipient contact verification delivery dispatch fence changed';
        end if;
    elsif old.state = 'dispatching'
          and new.state in ('delivered', 'rejected', 'indeterminate') then
        if new.fence_token <> old.fence_token
           or new.reserved_at <> old.reserved_at
           or new.lease_expires_at <> old.lease_expires_at
           or new.dispatch_started_at <> old.dispatch_started_at then
            raise exception 'Recipient contact verification delivery terminal fence changed';
        end if;
    else
        raise exception 'Recipient contact verification delivery transition is invalid';
    end if;
    return new;
end
$$;

create trigger recipient_contact_verification_deliveries_validate_transition
before update on recipient_contact_verification_deliveries
for each row execute function validate_recipient_contact_verification_delivery_transition();

create function reject_recipient_contact_verification_delivery_delete()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Recipient contact verification deliveries cannot be deleted';
end
$$;

create trigger recipient_contact_verification_deliveries_reject_delete
before delete on recipient_contact_verification_deliveries
for each row execute function reject_recipient_contact_verification_delivery_delete();

comment on table recipient_contact_verification_deliveries is
    'Identity-owned one-shot SMTP dispatch fences; mailbox, proof, message bytes, credentials, and provider response text are forbidden';
