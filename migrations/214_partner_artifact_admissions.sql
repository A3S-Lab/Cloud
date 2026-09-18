-- ArtifactAdmission receipts for partner-produced content.
-- Cloud stores digest + metadata only. Partner blob bytes are not stored.

create table partner_artifact_admissions (
    id uuid primary key,
    organization_id uuid not null references organizations(id),
    content_digest text not null,
    kind text not null check (kind in ('model', 'git', 'oci', 'generic')),
    byte_size bigint not null check (byte_size > 0 and byte_size <= 8796093022208),
    partner_ref text not null check (char_length(partner_ref) between 1 and 256),
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    constraint partner_artifact_admissions_digest_chk check (
        content_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    constraint partner_artifact_admissions_org_digest_unique
        unique (organization_id, content_digest)
);

create index partner_artifact_admissions_org_created_idx
    on partner_artifact_admissions (organization_id, created_at desc, id);
