create table workload_runtime_evidence_history (
    record_schema text not null check (
        record_schema = 'cloud.identity.workload-runtime-evidence-record.v1'
    ),
    binding_schema text not null check (
        binding_schema = 'cloud.identity.workload-runtime-evidence-binding.v1'
    ),
    binding_id uuid primary key check (
        binding_id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    installation_id uuid not null,
    organization_id uuid not null,
    project_id uuid not null,
    environment_id uuid not null,
    workload_id uuid not null,
    workload_revision_id uuid not null,
    policy_id uuid not null,
    policy_revision_id uuid not null,
    policy_revision_number bigint not null check (
        policy_revision_number between 1 and 9007199254740991
    ),
    policy_digest text not null check (
        policy_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    resource_claim_id uuid not null,
    resource_claim_generation bigint not null check (
        resource_claim_generation between 1 and 9007199254740991
    ),
    resource_claim_aggregate_version bigint not null check (
        resource_claim_aggregate_version between 1 and 9007199254740991
    ),
    resource_claim_digest text not null check (
        resource_claim_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    resource_binding_digest text not null check (
        resource_binding_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    node_pool_id uuid not null,
    node_pool_aggregate_version bigint not null check (
        node_pool_aggregate_version between 1 and 9007199254740991
    ),
    node_pool_spec_digest text not null check (
        node_pool_spec_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    node_id uuid not null,
    node_aggregate_version bigint not null check (
        node_aggregate_version between 1 and 9007199254740991
    ),
    agent_instance_id uuid not null,
    node_capabilities_digest text not null check (
        node_capabilities_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    node_last_observed_at timestamptz not null,
    runtime_report_id uuid not null,
    runtime_unit_id text not null check (
        char_length(runtime_unit_id) between 1 and 512
        and position(chr(10) in runtime_unit_id) = 0
        and position(chr(13) in runtime_unit_id) = 0
    ),
    runtime_generation bigint not null check (
        runtime_generation between 1 and 9007199254740991
    ),
    runtime_class text not null check (
        runtime_class in ('task', 'service')
    ),
    isolation_level text not null check (
        isolation_level in ('process', 'container', 'sandbox', 'confidential')
    ),
    semantics_profile_digest text not null check (
        semantics_profile_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    identity_attachment_digest text not null check (
        identity_attachment_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    runtime_spec_digest text not null check (
        runtime_spec_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    runtime_attestation_binding_digest text not null check (
        runtime_attestation_binding_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    provider_attestation_digest text not null check (
        provider_attestation_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    provider_resource_id text not null check (
        char_length(provider_resource_id) between 1 and 1024
        and position(chr(10) in provider_resource_id) = 0
        and position(chr(13) in provider_resource_id) = 0
    ),
    provider_build text not null check (
        char_length(provider_build) between 1 and 255
        and position(chr(10) in provider_build) = 0
        and position(chr(13) in provider_build) = 0
    ),
    runtime_state text not null check (runtime_state = 'running'),
    runtime_observed_at timestamptz not null,
    runtime_received_at timestamptz not null,
    node_attestation_binding_digest text check (
        node_attestation_binding_digest is null
        or node_attestation_binding_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    binding_digest text not null check (
        binding_digest ~ '^sha256:[0-9a-f]{64}$'
    ),
    admitted_at timestamptz not null,
    foreign key (organization_id, policy_id, policy_revision_id)
        references workload_identity_policy_revisions (organization_id, policy_id, id),
    check (identity_attachment_digest = policy_digest),
    check (node_attestation_binding_digest is null),
    check (runtime_received_at >= runtime_observed_at),
    check (node_last_observed_at >= runtime_observed_at),
    check (admitted_at >= runtime_received_at),
    check (admitted_at >= node_last_observed_at),
    check (admitted_at - runtime_observed_at <= interval '120 seconds'),
    check (admitted_at - node_last_observed_at <= interval '120 seconds')
);

create function validate_workload_runtime_evidence_history_insert()
returns trigger
language plpgsql
as $$
declare
    accepted_policy workload_identity_policy_revisions%rowtype;
begin
    if tg_op <> 'INSERT' then
        raise exception 'workload Runtime evidence history is immutable'
            using errcode = '23514';
    end if;

    perform 1
      from cloud_installations installation
     where installation.id = new.installation_id
       and installation.singleton_key
       for key share of installation;
    if not found then
        raise exception 'workload Runtime evidence has no canonical Installation'
            using errcode = '23503';
    end if;

    select policy_revision.*
      into accepted_policy
      from workload_identity_policy_revisions policy_revision
      join workload_identity_policy_heads policy_head
        on policy_head.installation_id = policy_revision.installation_id
       and policy_head.organization_id = policy_revision.organization_id
       and policy_head.policy_id = policy_revision.policy_id
       and policy_head.revision_id = policy_revision.id
       and policy_head.revision_number = policy_revision.revision_number
      join trust_domain_heads trust_head
        on trust_head.installation_id = policy_revision.installation_id
       and trust_head.trust_domain_id = policy_revision.trust_domain_id
       and trust_head.revision_id = policy_revision.trust_domain_revision_id
      join trust_domain_revisions trust_revision
        on trust_revision.installation_id = trust_head.installation_id
       and trust_revision.trust_domain_id = trust_head.trust_domain_id
       and trust_revision.id = trust_head.revision_id
     where policy_revision.installation_id = new.installation_id
       and policy_revision.organization_id = new.organization_id
       and policy_revision.project_id = new.project_id
       and policy_revision.environment_id = new.environment_id
       and policy_revision.policy_id = new.policy_id
       and policy_revision.workload_id = new.workload_id
       and policy_revision.workload_revision_id = new.workload_revision_id
       and policy_revision.node_pool_id = new.node_pool_id
       and policy_revision.id = new.policy_revision_id
       and policy_revision.revision_number = new.policy_revision_number
       and policy_revision.digest = new.policy_digest
       and policy_revision.digest = new.identity_attachment_digest
       for key share of policy_revision, policy_head, trust_head, trust_revision;
    if not found or new.admitted_at < accepted_policy.accepted_at then
        raise exception 'workload Runtime evidence must bind the exact current Policy and TrustDomain'
            using errcode = '23514';
    end if;
    return new;
end
$$;

create trigger workload_runtime_evidence_history_immutable_admission
before insert or update or delete on workload_runtime_evidence_history
for each row execute function validate_workload_runtime_evidence_history_insert();

create index workload_runtime_evidence_history_workload_idx
    on workload_runtime_evidence_history (
        installation_id,
        organization_id,
        workload_id,
        admitted_at desc,
        binding_id desc
    );

create index workload_runtime_evidence_history_policy_idx
    on workload_runtime_evidence_history (
        organization_id,
        policy_id,
        policy_revision_number desc,
        policy_revision_id
    );

comment on table workload_runtime_evidence_history is
    'Identity-owned immutable normalized Runtime evidence history; V1 has no hardware attestation and never authorizes workload credential issuance';
