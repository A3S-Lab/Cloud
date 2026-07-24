alter table workload_revisions
    add constraint workload_revisions_route_target_identity_key
    unique (workload_id, id, generation);

alter table routes
    add column runtime_unit_id text,
    add column runtime_generation bigint,
    add column target_observed_at timestamptz;

update routes as route
set runtime_unit_id =
        'workload:' || route.workload_id::text
            || ':revision:' || route.workload_revision_id::text,
    runtime_generation = revision.generation,
    target_observed_at = route.updated_at
from workload_revisions as revision
where revision.workload_id = route.workload_id
    and revision.id = route.workload_revision_id;

alter table routes
    alter column runtime_unit_id set not null,
    alter column runtime_generation set not null,
    alter column target_observed_at set not null,
    add constraint routes_runtime_generation_check
        check (runtime_generation > 0),
    add constraint routes_runtime_unit_identity_check
        check (
            runtime_unit_id =
                'workload:' || workload_id::text
                    || ':revision:' || workload_revision_id::text
        ),
    add constraint routes_target_observation_time_check
        check (target_observed_at <= updated_at),
    add constraint routes_target_revision_generation_fk
        foreign key (workload_id, workload_revision_id, runtime_generation)
        references workload_revisions (workload_id, id, generation);

alter table gateway_route_cutovers
    add column previous_generation bigint,
    add column candidate_generation bigint;

update gateway_route_cutovers as cutover
set previous_generation = previous_revision.generation,
    candidate_generation = candidate_revision.generation
from workload_revisions as previous_revision,
    workload_revisions as candidate_revision
where previous_revision.workload_id = cutover.workload_id
    and previous_revision.id = cutover.previous_revision_id
    and candidate_revision.workload_id = cutover.workload_id
    and candidate_revision.id = cutover.candidate_revision_id;

update gateway_route_cutovers as cutover
set routes = (
    select jsonb_agg(
        route.document
            || jsonb_build_object(
                'runtime_unit_id',
                'workload:' || (route.document ->> 'workload_id')
                    || ':revision:' || (route.document ->> 'workload_revision_id'),
                'runtime_generation',
                revision.generation,
                'observed_at',
                route.document -> 'updated_at'
            )
        order by route.ordinality
    )
    from jsonb_array_elements(cutover.routes)
        with ordinality as route(document, ordinality)
    join workload_revisions as revision
        on revision.workload_id = (route.document ->> 'workload_id')::uuid
        and revision.id = (route.document ->> 'workload_revision_id')::uuid
);

alter table gateway_route_cutovers
    alter column previous_generation set not null,
    alter column candidate_generation set not null,
    add constraint gateway_route_cutovers_generation_order_check
        check (
            previous_generation > 0
                and candidate_generation > previous_generation
        ),
    add constraint gateway_route_cutovers_previous_target_fk
        foreign key (workload_id, previous_revision_id, previous_generation)
        references workload_revisions (workload_id, id, generation),
    add constraint gateway_route_cutovers_candidate_target_fk
        foreign key (workload_id, candidate_revision_id, candidate_generation)
        references workload_revisions (workload_id, id, generation);
