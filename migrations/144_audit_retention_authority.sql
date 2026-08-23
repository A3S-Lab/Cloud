create table audit_retention_states (
    organization_id uuid primary key references organizations(id) on delete cascade,
    records_available_from timestamptz,
    records_deleted_before timestamptz,
    applied_policy_digest text,
    total_deleted_records bigint not null default 0 check (total_deleted_records >= 0),
    last_swept_at timestamptz,
    last_completed_at timestamptz,
    next_scan_at timestamptz not null default '1970-01-01 00:00:00+00',
    version bigint not null default 0 check (version >= 0),
    check (
        applied_policy_digest is null
        or applied_policy_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    check (
        records_deleted_before is null
        or (
            records_available_from is not null
            and records_deleted_before <= records_available_from
        )
    ),
    check (last_completed_at is null or records_deleted_before is not null),
    check (
        (
            records_available_from is null
            and applied_policy_digest is null
            and last_swept_at is null
        )
        or (
            records_available_from is not null
            and applied_policy_digest is not null
            and last_swept_at is not null
        )
    ),
    check (last_swept_at is null or next_scan_at > last_swept_at),
    check (
        last_completed_at is null
        or (
            last_swept_at is not null
            and last_completed_at <= last_swept_at
        )
    )
);

insert into audit_retention_states (organization_id)
select id from organizations;

create function create_audit_retention_state_for_organization()
returns trigger
language plpgsql
as $$
begin
    insert into audit_retention_states (organization_id)
    values (new.id);
    return new;
end
$$;

create trigger organizations_create_audit_retention_state
after insert on organizations
for each row execute function create_audit_retention_state_for_organization();

create function enforce_audit_retention_state_monotonicity()
returns trigger
language plpgsql
as $$
begin
    if new.organization_id is distinct from old.organization_id
       or (old.records_available_from is not null and (
            new.records_available_from is null
            or new.records_available_from < old.records_available_from
       ))
       or (old.records_deleted_before is not null and (
            new.records_deleted_before is null
            or new.records_deleted_before < old.records_deleted_before
       ))
       or new.total_deleted_records < old.total_deleted_records
       or (old.applied_policy_digest is not null and new.applied_policy_digest is null)
       or (old.last_swept_at is not null and (
            new.last_swept_at is null
            or new.last_swept_at < old.last_swept_at
       ))
       or (old.last_completed_at is not null and (
            new.last_completed_at is null
            or new.last_completed_at < old.last_completed_at
       ))
       or new.next_scan_at < old.next_scan_at
       or new.version <> old.version + 1 then
        raise exception 'audit retention state must advance monotonically'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger audit_retention_states_monotonic
before update on audit_retention_states
for each row execute function enforce_audit_retention_state_monotonicity();

create function reject_audit_record_before_retention_boundary()
returns trigger
language plpgsql
as $$
declare
    retained_from timestamptz;
begin
    select state.records_available_from
      into retained_from
      from audit_retention_states state
     where state.organization_id = new.organization_id
       for share;
    if not found then
        raise exception 'audit record organization has no retention authority'
            using errcode = '23503';
    end if;
    if retained_from is not null and new.occurred_at < retained_from then
        raise exception 'audit record is older than the retained availability boundary'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger audit_records_enforce_retention_boundary
before insert on audit_records
for each row execute function reject_audit_record_before_retention_boundary();

create index audit_retention_states_next_scan_idx
    on audit_retention_states (next_scan_at, organization_id);

comment on table audit_retention_states is
    'Per-organization monotonic audit availability and bounded physical-deletion authority';
comment on column audit_retention_states.records_available_from is
    'Inclusive lower bound exposed by audit queries; records below it are unavailable even while bounded physical cleanup catches up';
comment on column audit_retention_states.records_deleted_before is
    'Exclusive upper bound for which physical deletion has completed';
