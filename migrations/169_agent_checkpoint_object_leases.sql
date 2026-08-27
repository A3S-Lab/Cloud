create table agent_execution_checkpoint_object_leases (
    object_ref text primary key check (
        octet_length(object_ref) between 1 and 4096
        and position(chr(92) in object_ref) = 0
        and position(chr(13) in object_ref) = 0
        and position(chr(10) in object_ref) = 0
    ),
    organization_id uuid not null,
    execution_id uuid not null,
    checkpoint_id uuid not null,
    object_digest text not null check (object_digest ~ '^sha256:[0-9a-f]{64}$'),
    object_size_bytes bigint not null check (object_size_bytes between 1 and 917504),
    purpose text not null check (purpose in ('capture', 'inventory', 'cleanup')),
    lease_id uuid not null,
    reserved_at timestamptz not null,
    lease_expires_at timestamptz not null check (lease_expires_at > reserved_at),
    check (
        object_ref = 'organizations/' || organization_id::text
            || '/executions/' || execution_id::text
            || '/checkpoints/' || checkpoint_id::text
            || '/sha256/' || substring(object_digest from 8)
            || '/checkpoint.json'
    )
);

create index agent_execution_checkpoint_object_leases_expiration_idx
    on agent_execution_checkpoint_object_leases (
        lease_expires_at,
        object_ref
    );

comment on table agent_execution_checkpoint_object_leases is
    'Short-lived fencing authority for A1.6 checkpoint object capture, inventory grace, and cleanup; it stores no checkpoint payload and deliberately has no tenant foreign key so deleted-tenant or legacy orphan objects remain reclaimable';
