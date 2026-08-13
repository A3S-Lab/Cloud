alter table organization_memberships
    add constraint organization_memberships_invitation_identity_unique
        unique (organization_id, id, principal_id);

create table membership_invitations (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    principal_id uuid not null references identity_principals(id),
    role text not null check (role in ('owner', 'admin', 'member', 'restricted')),
    invited_by_principal_id uuid not null references identity_principals(id),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    expires_at timestamptz not null,
    accepted_membership_id uuid,
    accepted_at timestamptz,
    revoked_at timestamptz,
    foreign key (organization_id, accepted_membership_id, principal_id)
        references organization_memberships (organization_id, id, principal_id),
    check (updated_at >= created_at),
    check (expires_at > created_at),
    check (expires_at <= created_at + interval '30 days'),
    check ((accepted_membership_id is null) = (accepted_at is null)),
    check (accepted_at is null or accepted_at = updated_at),
    check (accepted_at is null or accepted_at < expires_at),
    check (revoked_at is null or revoked_at = updated_at),
    check (revoked_at is null or revoked_at < expires_at),
    check (accepted_at is null or revoked_at is null)
);

create index membership_invitations_organization_history_idx
    on membership_invitations (organization_id, created_at, id);

create index membership_invitations_principal_history_idx
    on membership_invitations (principal_id, created_at, id);

create index membership_invitations_pending_idx
    on membership_invitations (organization_id, principal_id, expires_at, id)
    where accepted_at is null and revoked_at is null;

create or replace function enforce_membership_invitation_transition()
returns trigger
language plpgsql
as $$
begin
    if new.id <> old.id
        or new.organization_id <> old.organization_id
        or new.principal_id <> old.principal_id
        or new.role <> old.role
        or new.invited_by_principal_id <> old.invited_by_principal_id
        or new.created_at <> old.created_at
        or new.expires_at <> old.expires_at then
        raise exception 'membership invitation identity is immutable';
    end if;

    if old.accepted_at is not null or old.revoked_at is not null then
        if new is distinct from old then
            raise exception 'terminal membership invitation history is immutable';
        end if;
        return new;
    end if;

    if new.aggregate_version <> old.aggregate_version + 1
        or new.updated_at < old.updated_at
        or not (
            (
                new.accepted_membership_id is not null
                and new.accepted_at is not null
                and new.revoked_at is null
            )
            or (
                new.accepted_membership_id is null
                and new.accepted_at is null
                and new.revoked_at is not null
            )
        ) then
        raise exception 'membership invitation transition is invalid';
    end if;
    return new;
end
$$;

create trigger membership_invitations_enforce_transition
before update on membership_invitations
for each row execute function enforce_membership_invitation_transition();

create or replace function reject_membership_invitation_delete()
returns trigger
language plpgsql
as $$
begin
    raise exception 'membership invitation history is immutable';
end
$$;

create trigger membership_invitations_no_delete
before delete on membership_invitations
for each row execute function reject_membership_invitation_delete();
