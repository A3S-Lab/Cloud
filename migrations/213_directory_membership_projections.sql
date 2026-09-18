-- DirectoryProjection membership bindings: which Principals belong to which
-- opaque department/group subject refs. Partner directory trees are not stored.

create table directory_membership_projections (
    organization_id uuid not null references organizations(id),
    subject_kind text not null check (subject_kind in ('department', 'group')),
    directory_issuer text not null,
    directory_subject_id uuid not null,
    principal_id uuid not null references identity_principals(id),
    created_at timestamptz not null,
    primary key (
        organization_id,
        subject_kind,
        directory_issuer,
        directory_subject_id,
        principal_id
    )
);

create index directory_membership_projections_principal_idx
    on directory_membership_projections (organization_id, principal_id);

create index directory_membership_projections_subject_idx
    on directory_membership_projections (
        organization_id,
        subject_kind,
        directory_issuer,
        directory_subject_id
    );
