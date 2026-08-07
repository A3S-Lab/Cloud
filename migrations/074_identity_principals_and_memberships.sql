create table identity_principals (
    id uuid primary key,
    kind text not null check (kind in ('human', 'service')),
    name text not null check (char_length(name) between 1 and 63),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    disabled_at timestamptz,
    check (disabled_at is null or disabled_at >= created_at)
);

alter table api_tokens
    add column principal_id uuid;

insert into identity_principals (
    id,
    kind,
    name,
    aggregate_version,
    created_at,
    disabled_at
)
select
    id,
    'service',
    name,
    1,
    created_at,
    null
from api_tokens;

update api_tokens
set principal_id = id;

alter table api_tokens
    alter column principal_id set not null,
    add constraint api_tokens_principal_fk
        foreign key (principal_id) references identity_principals(id);

create index api_tokens_principal_idx
    on api_tokens (principal_id, created_at, id);

create table organization_memberships (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    principal_id uuid not null references identity_principals(id),
    role text not null check (role in ('owner', 'admin', 'member', 'restricted')),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    revoked_at timestamptz,
    unique (organization_id, principal_id),
    check (updated_at >= created_at),
    check (revoked_at is null or revoked_at = updated_at)
);

with ranked_tokens as (
    select
        id,
        organization_id,
        principal_id,
        scopes,
        created_at,
        row_number() over (
            partition by organization_id
            order by
                case
                    when revoked_at is null
                        and (expires_at is null or expires_at > now()) then 0
                    else 1
                end,
                case
                    when scopes ? 'platform:write' then 0
                    when scopes ? 'token:write' then 1
                    else 2
                end,
                created_at,
                id
        ) as organization_rank
    from api_tokens
)
insert into organization_memberships (
    id,
    organization_id,
    principal_id,
    role,
    aggregate_version,
    created_at,
    updated_at,
    revoked_at
)
select
    id,
    organization_id,
    principal_id,
    case
        when organization_rank = 1 then 'owner'
        when scopes ? 'platform:write' or scopes ? 'token:write' then 'admin'
        else 'member'
    end,
    1,
    created_at,
    created_at,
    null
from ranked_tokens;

insert into organization_memberships (
    id,
    organization_id,
    principal_id,
    role,
    aggregate_version,
    created_at,
    updated_at,
    revoked_at
)
select
    md5('a3s-cloud.owner-membership:' || organization.id::text)::uuid,
    organization.id,
    platform_principal.principal_id,
    'owner',
    1,
    organization.created_at,
    organization.created_at,
    null
from organizations organization
cross join lateral (
    select principal_id
    from api_tokens
    where scopes ? 'platform:write'
    order by
        case
            when revoked_at is null
                and (expires_at is null or expires_at > now()) then 0
            else 1
        end,
        created_at,
        id
    limit 1
) platform_principal
where not exists (
    select 1
    from organization_memberships membership
    where membership.organization_id = organization.id
);

do $$
begin
    if exists (
        select 1
        from organizations organization
        where not exists (
            select 1
            from organization_memberships membership
            where membership.organization_id = organization.id
              and membership.role = 'owner'
              and membership.revoked_at is null
        )
    ) then
        raise exception 'identity migration could not assign an owner to every organization';
    end if;
end
$$;

create index organization_memberships_principal_idx
    on organization_memberships (principal_id, organization_id);

create index organization_memberships_active_idx
    on organization_memberships (organization_id, role, created_at, id)
    where revoked_at is null;

update api_tokens
set scopes = scopes || '["identity:write"]'::jsonb
where principal_id in (
    select principal_id
    from organization_memberships
    where role in ('owner', 'admin')
)
  and not scopes ? 'identity:write';
