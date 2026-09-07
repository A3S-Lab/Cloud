-- U0.3: retain the exact digest-bound confirmation envelope so Fleet apply can
-- carry canonical confirmation without reconstructing it from digests alone.

alter table plugin_plan_projections
    add column confirmation jsonb;

alter table plugin_plan_projections
    add constraint plugin_plan_projections_confirmation_pair_chk check (
        (confirmation_digest is null and confirmation is null)
        or (
            confirmation_digest is not null
            and confirmation is not null
            and jsonb_typeof(confirmation) = 'object'
        )
    );
