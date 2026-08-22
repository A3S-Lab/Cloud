create table recipient_contacts (
    id uuid primary key,
    principal_id uuid not null references identity_principals(id),
    canonical_address text not null check (
        char_length(canonical_address) between 3 and 254
        and octet_length(canonical_address) = char_length(canonical_address)
        and canonical_address = lower(btrim(canonical_address))
        and canonical_address !~ '[[:space:][:cntrl:]]'
        and length(canonical_address) - length(replace(canonical_address, '@', '')) = 1
        and char_length(split_part(canonical_address, '@', 1)) between 1 and 64
        and canonical_address ~ '^[a-z0-9!#$%&''*+/=?^_`{|}~-]+([.][a-z0-9!#$%&''*+/=?^_`{|}~-]+)*@[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?([.][a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*$'
    ),
    address_digest text not null check (address_digest ~ '^sha256:[0-9a-f]{64}$'),
    aggregate_version bigint not null check (aggregate_version > 0),
    state text not null check (state in ('pending', 'verified', 'revoked')),
    created_at timestamptz not null,
    updated_at timestamptz not null check (updated_at >= created_at),
    verified_at timestamptz,
    revoked_at timestamptz,
    unique (principal_id, canonical_address),
    unique (principal_id, id),
    check (
        (state = 'pending' and aggregate_version = 1 and updated_at = created_at
            and verified_at is null and revoked_at is null)
        or (state = 'verified' and verified_at = updated_at and revoked_at is null)
        or (state = 'revoked' and revoked_at = updated_at)
    )
);

create index recipient_contacts_principal_created_idx
    on recipient_contacts (principal_id, created_at, id);

create table recipient_contact_verifications (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    contact_id uuid not null,
    principal_id uuid not null,
    address_digest text not null check (address_digest ~ '^sha256:[0-9a-f]{64}$'),
    contact_version bigint not null check (contact_version > 0),
    signing_key_id text not null check (
        char_length(signing_key_id) between 1 and 64
        and signing_key_id = lower(btrim(signing_key_id))
        and signing_key_id ~ '^[a-z]([a-z0-9._-]*[a-z0-9])?$'
    ),
    issued_at timestamptz not null,
    expires_at timestamptz not null check (
        expires_at >= issued_at + interval '1 minute'
        and expires_at <= issued_at + interval '30 minutes'
    ),
    consumed_at timestamptz,
    invalidated_at timestamptz,
    foreign key (principal_id, contact_id)
        references recipient_contacts (principal_id, id),
    check (consumed_at is null or (consumed_at >= issued_at and consumed_at < expires_at)),
    check (invalidated_at is null or invalidated_at >= issued_at),
    check (consumed_at is null or invalidated_at is null)
);

create unique index recipient_contact_verifications_pending_idx
    on recipient_contact_verifications (contact_id)
    where consumed_at is null and invalidated_at is null;

create index recipient_contact_verifications_owner_history_idx
    on recipient_contact_verifications (principal_id, contact_id, issued_at desc, id desc);

create function validate_recipient_contact_principal()
returns trigger
language plpgsql
as $$
begin
    if not exists (
        select 1
          from identity_principals
         where id = new.principal_id
           and kind = 'human'
           and disabled_at is null
    ) then
        raise exception 'Recipient contacts require an active human identity principal';
    end if;
    return new;
end
$$;

create trigger recipient_contacts_validate_principal
before insert on recipient_contacts
for each row execute function validate_recipient_contact_principal();

create function validate_recipient_contact_transition()
returns trigger
language plpgsql
as $$
begin
    if new.id <> old.id
       or new.principal_id <> old.principal_id
       or new.canonical_address <> old.canonical_address
       or new.address_digest <> old.address_digest
       or new.created_at <> old.created_at then
        raise exception 'Recipient contact identity and address are immutable';
    end if;
    if old.state = 'revoked' then
        raise exception 'Revoked recipient contacts are terminal';
    end if;
    if new.aggregate_version <> old.aggregate_version + 1
       or new.updated_at < old.updated_at then
        raise exception 'Recipient contact transition version is invalid';
    end if;
    if old.state = 'pending' and new.state = 'verified' then
        if new.verified_at is null
           or new.revoked_at is not null then
            raise exception 'Recipient contact verification transition is invalid';
        end if;
    elsif new.state = 'revoked' then
        if new.revoked_at is null
           or new.verified_at is distinct from old.verified_at then
            raise exception 'Recipient contact revocation transition is invalid';
        end if;
    else
        raise exception 'Recipient contact state transition is invalid';
    end if;
    return new;
end
$$;

create trigger recipient_contacts_validate_transition
before update on recipient_contacts
for each row execute function validate_recipient_contact_transition();

create function reject_recipient_contact_delete()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Recipient contacts cannot be deleted';
end
$$;

create trigger recipient_contacts_reject_delete
before delete on recipient_contacts
for each row execute function reject_recipient_contact_delete();

create function validate_recipient_contact_verification_insert()
returns trigger
language plpgsql
as $$
begin
    if not exists (
        select 1
          from recipient_contacts
         where id = new.contact_id
           and principal_id = new.principal_id
           and address_digest = new.address_digest
           and aggregate_version = new.contact_version
           and state = 'pending'
    ) then
        raise exception 'Recipient contact verification must bind the current pending contact';
    end if;
    if not exists (
        select 1
          from organization_memberships
         where organization_id = new.organization_id
           and principal_id = new.principal_id
           and revoked_at is null
    ) then
        raise exception 'Recipient contact verification requires an active organization membership';
    end if;
    return new;
end
$$;

create trigger recipient_contact_verifications_validate_insert
before insert on recipient_contact_verifications
for each row execute function validate_recipient_contact_verification_insert();

create function validate_recipient_contact_verification_transition()
returns trigger
language plpgsql
as $$
begin
    if new.id <> old.id
       or new.organization_id <> old.organization_id
       or new.contact_id <> old.contact_id
       or new.principal_id <> old.principal_id
       or new.address_digest <> old.address_digest
       or new.contact_version <> old.contact_version
       or new.signing_key_id <> old.signing_key_id
       or new.issued_at <> old.issued_at
       or new.expires_at <> old.expires_at then
        raise exception 'Recipient contact verification identity is immutable';
    end if;
    if old.consumed_at is not null or old.invalidated_at is not null then
        raise exception 'Recipient contact verification terminal state is immutable';
    end if;
    if (new.consumed_at is null) = (new.invalidated_at is null) then
        raise exception 'Recipient contact verification transition is invalid';
    end if;
    return new;
end
$$;

create trigger recipient_contact_verifications_validate_transition
before update on recipient_contact_verifications
for each row execute function validate_recipient_contact_verification_transition();

create function reject_recipient_contact_verification_delete()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Recipient contact verifications cannot be deleted';
end
$$;

create trigger recipient_contact_verifications_reject_delete
before delete on recipient_contact_verifications
for each row execute function reject_recipient_contact_verification_delete();

comment on table recipient_contacts is
    'Identity-owned exact-human recipient contacts; canonical mailboxes are PII and never copied into public projections, Outbox facts, or audit details';

comment on table recipient_contact_verifications is
    'Short-lived single-use recipient-contact proof bindings; proof and signature material are never persisted';
