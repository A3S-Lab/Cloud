create table connector_revision_revocations (
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    profile_id uuid not null,
    revision_id uuid not null,
    revision_number bigint not null check (revision_number > 0),
    definition_digest text not null
        check (definition_digest ~ '^sha256:[0-9a-f]{64}$'),
    reason text not null
        check (
            octet_length(reason) between 1 and 1024
            and reason = btrim(reason)
            and reason !~ '[[:cntrl:]]'
        ),
    revoked_by uuid not null references identity_principals(id),
    revoked_at timestamptz not null,
    primary key (organization_id, profile_id, revision_id),
    unique (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id
    ),
    foreign key (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id
    ) references connector_revisions (
        organization_id,
        project_id,
        environment_id,
        profile_id,
        id
    ),
    foreign key (
        organization_id,
        profile_id,
        revision_id,
        definition_digest
    ) references connector_revisions (
        organization_id,
        profile_id,
        id,
        definition_digest
    )
);

create index connector_revision_revocations_environment_time_idx
    on connector_revision_revocations (
        organization_id,
        project_id,
        environment_id,
        revoked_at desc,
        revision_id
    );

create function validate_connector_revision_revocation()
returns trigger
language plpgsql
as $$
declare
    stored_revision_number bigint;
    stored_definition_digest text;
    stored_created_at timestamptz;
begin
    select revision_number, definition_digest, created_at
      into stored_revision_number, stored_definition_digest, stored_created_at
      from connector_revisions
     where organization_id = new.organization_id
       and project_id = new.project_id
       and environment_id = new.environment_id
       and profile_id = new.profile_id
       and id = new.revision_id
     for update;

    if not found
       or new.revision_number <> stored_revision_number
       or new.definition_digest <> stored_definition_digest
       or new.revoked_at < stored_created_at then
        raise exception 'Connector revision revocation does not match its exact revision';
    end if;
    return new;
end
$$;

create trigger connector_revision_revocations_validate
before insert on connector_revision_revocations
for each row execute function validate_connector_revision_revocation();

create function reject_connector_revision_revocation_mutation()
returns trigger
language plpgsql
as $$
begin
    raise exception 'Connector revision revocations are immutable';
end
$$;

create trigger connector_revision_revocations_immutable
before update or delete on connector_revision_revocations
for each row execute function reject_connector_revision_revocation_mutation();

comment on table connector_revision_revocations is
    'Immutable exact Connector revision revocation facts serialized with provider dispatch admission';

comment on column connector_revision_revocations.reason is
    'Bounded operator reason retained for audit and API reads; never provider or Secret material';
