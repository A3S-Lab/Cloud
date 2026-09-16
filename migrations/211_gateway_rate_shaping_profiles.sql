create table gateway_rate_shaping_profiles (
    profile_id text primary key
        check (char_length(profile_id) between 1 and 128),
    policy_revision_digest text not null
        check (policy_revision_digest ~ '^sha256:[0-9a-f]{64}$'),
    algorithm_kind text not null
        check (algorithm_kind in ('token_bucket', 'gcra')),
    token_bucket_capacity bigint,
    token_bucket_refill_tokens_per_second bigint,
    gcra_emission_interval_nanos bigint,
    gcra_burst_tolerance bigint,
    updated_at timestamptz not null default now(),
    check (
        (
            algorithm_kind = 'token_bucket'
            and token_bucket_capacity is not null
            and token_bucket_capacity > 0
            and token_bucket_refill_tokens_per_second is not null
            and token_bucket_refill_tokens_per_second > 0
            and gcra_emission_interval_nanos is null
            and gcra_burst_tolerance is null
        )
        or (
            algorithm_kind = 'gcra'
            and gcra_emission_interval_nanos is not null
            and gcra_emission_interval_nanos > 0
            and gcra_burst_tolerance is not null
            and gcra_burst_tolerance > 0
            and token_bucket_capacity is null
            and token_bucket_refill_tokens_per_second is null
        )
    )
);

comment on table gateway_rate_shaping_profiles is
    'Edge/Gateway-owned durable rate-shaping profile catalog revisions; upserted by ACL seed and CQRS register; Applications declare only opaque profile digests';
