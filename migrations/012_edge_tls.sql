create table domain_claims (
    id uuid primary key,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    pattern text not null,
    challenge_dns_name text not null,
    challenge_value text not null,
    state text not null check (state in ('pending', 'verified', 'rejected', 'revoked')),
    failure text,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    verified_at timestamptz,
    revoked_at timestamptz,
    foreign key (organization_id, project_id, environment_id)
        references environments (organization_id, project_id, id),
    check (updated_at >= created_at),
    check (
        state = 'pending'
            and failure is null and verified_at is null and revoked_at is null
        or state = 'verified'
            and failure is null and verified_at is not null and revoked_at is null
        or state = 'rejected'
            and failure is not null and verified_at is null and revoked_at is null
        or state = 'revoked'
            and failure is not null and verified_at is not null and revoked_at is not null
    )
);

create unique index domain_claims_active_pattern_idx
    on domain_claims (pattern)
    where state in ('pending', 'verified');

create index domain_claims_environment_idx
    on domain_claims (organization_id, project_id, environment_id, created_at, id);

alter table gateway_publications
    add column certificate_request jsonb;

create table gateway_certificates (
    id uuid primary key,
    organization_id uuid not null,
    node_id uuid not null,
    domain_claim_ids jsonb not null,
    gateway_revision bigint not null check (gateway_revision > 0),
    gateway_command_id uuid not null,
    snapshot_digest text not null check (snapshot_digest ~ '^sha256:[0-9a-f]{64}$'),
    request jsonb not null,
    state text not null check (state in ('provisioning', 'issued', 'ready', 'failed', 'revoked')),
    csr_digest text check (csr_digest is null or csr_digest ~ '^sha256:[0-9a-f]{64}$'),
    serial_number text,
    fingerprint text check (fingerprint is null or fingerprint ~ '^sha256:[0-9a-f]{64}$'),
    certificate_pem text,
    ca_bundle_pem text,
    issued_at timestamptz,
    expires_at timestamptz,
    failure text,
    aggregate_version bigint not null check (aggregate_version > 0),
    created_at timestamptz not null,
    updated_at timestamptz not null,
    ready_at timestamptz,
    revoked_at timestamptz,
    unique (node_id, gateway_revision),
    foreign key (organization_id, node_id)
        references nodes (organization_id, id),
    foreign key (node_id, gateway_revision, gateway_command_id)
        references gateway_publications (node_id, revision, command_id),
    check (jsonb_typeof(domain_claim_ids) = 'array' and jsonb_array_length(domain_claim_ids) > 0),
    check (jsonb_typeof(request) = 'object'),
    check (updated_at >= created_at),
    check (
        serial_number is null and fingerprint is null and certificate_pem is null
            and ca_bundle_pem is null and issued_at is null and expires_at is null
        or serial_number is not null and fingerprint is not null and certificate_pem is not null
            and ca_bundle_pem is not null and issued_at is not null and expires_at > issued_at
    ),
    check (
        state = 'provisioning'
            and csr_digest is null and serial_number is null and failure is null
            and ready_at is null and revoked_at is null
        or state = 'issued'
            and csr_digest is not null and serial_number is not null and failure is null
            and ready_at is null and revoked_at is null
        or state = 'ready'
            and csr_digest is not null and serial_number is not null and failure is null
            and ready_at is not null and revoked_at is null
        or state = 'failed'
            and failure is not null and ready_at is null and revoked_at is null
        or state = 'revoked'
            and csr_digest is not null and serial_number is not null and failure is not null
            and ready_at is not null and revoked_at is not null
    )
);

create index gateway_certificates_organization_idx
    on gateway_certificates (organization_id, created_at, id);

alter table routes
    add column domain_claim_id uuid references domain_claims(id),
    add column domain_pattern text,
    add column gateway_certificate_id uuid references gateway_certificates(id),
    add check (
        domain_claim_id is null and domain_pattern is null and gateway_certificate_id is null
        or domain_claim_id is not null
            and domain_pattern is not null
            and gateway_certificate_id is not null
    );

create index routes_domain_claim_idx
    on routes (domain_claim_id)
    where domain_claim_id is not null;
