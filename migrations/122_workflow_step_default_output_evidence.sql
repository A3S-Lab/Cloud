do $$
declare
    legacy_selected_handle_constraint text;
begin
    select constraint_record.conname
    into legacy_selected_handle_constraint
    from pg_constraint as constraint_record
    where constraint_record.conrelid = 'workflow_step_projections'::regclass
      and constraint_record.contype = 'c'
      and pg_get_constraintdef(constraint_record.oid) like '%kind%branch%selected_handle%'
    order by constraint_record.conname
    limit 1;

    if legacy_selected_handle_constraint is null then
        raise exception 'workflow_step_projections selected-handle constraint is missing';
    end if;

    execute format(
        'alter table workflow_step_projections drop constraint %I',
        legacy_selected_handle_constraint
    );
end
$$;

alter table workflow_step_projections
    add constraint workflow_step_projections_selected_handle_check check (
        selected_handle is null
        or kind = 'branch'
        or (
            kind = 'execution'
            and status = 'failed'
        )
    );

alter table workflow_step_projections
    add column default_output_evidence jsonb;

alter table workflow_step_projections
    add constraint workflow_step_default_output_evidence_shape check (
        default_output_evidence is null
        or (
            jsonb_typeof(default_output_evidence) = 'object'
            and octet_length(default_output_evidence::text) <= 262144
            and default_output_evidence ->> 'schema'
                = 'cloud.workflow.step-default-output.v1'
            and jsonb_typeof(default_output_evidence -> 'failure') = 'object'
            and default_output_evidence #>> '{failure,stepId}' = step_id
            and kind = 'execution'
            and status = 'completed'
            and result is not null
            and selected_handle is null
            and error is null
        ) is true
    );

comment on column workflow_step_projections.default_output_evidence is
    'Authority-bound terminal Execution failure observation folded into an exact immutable default output; nullable for ordinary completion and legacy replay.';
