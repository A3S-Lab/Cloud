create table external_identity_links (
    id uuid primary key,
    provider_key text not null check (
        octet_length(provider_key) between 1 and 63
        and (
            provider_key ~ '^[a-z]$'
            or provider_key ~ '^[a-z][a-z0-9_-]*[a-z0-9]$'
        )
    ),
    issuer text not null check (
        octet_length(issuer) between 1 and 2048
        and issuer like 'https://%'
    ),
    subject text not null check (
        octet_length(subject) between 1 and 255
        and position(chr(10) in subject) = 0
        and position(chr(13) in subject) = 0
    ),
    principal_id uuid not null references identity_principals(id),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    last_verified_at timestamptz not null,
    revoked_at timestamptz,
    unique (issuer, subject),
    check (last_verified_at >= created_at),
    check (revoked_at is null or revoked_at >= last_verified_at)
);

create index external_identity_links_principal_history_idx
    on external_identity_links (principal_id, created_at, id);

create unique index external_identity_links_active_principal_issuer_idx
    on external_identity_links (principal_id, issuer)
    where revoked_at is null;

create table oidc_flows (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    provider_key text not null check (
        octet_length(provider_key) between 1 and 63
        and (
            provider_key ~ '^[a-z]$'
            or provider_key ~ '^[a-z][a-z0-9_-]*[a-z0-9]$'
        )
    ),
    issuer text not null check (
        octet_length(issuer) between 1 and 2048
        and issuer like 'https://%'
    ),
    provider_config_digest text not null check (
        provider_config_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    purpose text not null check (purpose in ('login', 'link')),
    principal_id uuid references identity_principals(id),
    state_digest text not null unique check (state_digest ~ '^sha256:[0-9a-f]{64}$'),
    nonce_digest text not null unique check (nonce_digest ~ '^sha256:[0-9a-f]{64}$'),
    pkce_verifier_digest text not null unique check (pkce_verifier_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_at timestamptz not null,
    expires_at timestamptz not null,
    consumed_at timestamptz,
    foreign key (organization_id, principal_id)
        references organization_memberships (organization_id, principal_id),
    check ((purpose = 'link') = (principal_id is not null)),
    check (expires_at >= created_at + interval '1 minute'),
    check (expires_at <= created_at + interval '15 minutes'),
    check (consumed_at is null or (consumed_at >= created_at and consumed_at < expires_at))
);

create index oidc_flows_expiry_idx
    on oidc_flows (expires_at, id)
    where consumed_at is null;

create or replace function reject_oidc_flow_identity_change()
returns trigger
language plpgsql
as $$
begin
    if new.id <> old.id
        or new.organization_id <> old.organization_id
        or new.provider_key <> old.provider_key
        or new.issuer <> old.issuer
        or new.provider_config_digest <> old.provider_config_digest
        or new.purpose <> old.purpose
        or new.principal_id is distinct from old.principal_id
        or new.state_digest <> old.state_digest
        or new.nonce_digest <> old.nonce_digest
        or new.pkce_verifier_digest <> old.pkce_verifier_digest
        or new.created_at <> old.created_at
        or new.expires_at <> old.expires_at
        or old.consumed_at is not null
        or new.consumed_at is null then
        raise exception 'OIDC flow transition is invalid';
    end if;
    return new;
end
$$;

create trigger oidc_flows_consume_once
before update on oidc_flows
for each row execute function reject_oidc_flow_identity_change();

create or replace function reject_external_identity_delete()
returns trigger
language plpgsql
as $$
begin
    raise exception 'external identity history is immutable';
end
$$;

create trigger external_identity_links_no_delete
before delete on external_identity_links
for each row execute function reject_external_identity_delete();

create or replace function enforce_external_identity_link_transition()
returns trigger
language plpgsql
as $$
begin
    if new.id <> old.id
        or new.provider_key <> old.provider_key
        or new.issuer <> old.issuer
        or new.subject <> old.subject
        or new.principal_id <> old.principal_id
        or new.created_at <> old.created_at
        or old.revoked_at is not null
        or new.aggregate_version <> old.aggregate_version + 1
        or new.last_verified_at < old.last_verified_at
        or not (
            (
                new.revoked_at is null
                and new.last_verified_at > old.last_verified_at
            )
            or (
                new.revoked_at is not null
                and new.last_verified_at = old.last_verified_at
            )
        ) then
        raise exception 'external identity link transition is invalid';
    end if;
    return new;
end
$$;

create trigger external_identity_links_enforce_transition
before update on external_identity_links
for each row execute function enforce_external_identity_link_transition();
