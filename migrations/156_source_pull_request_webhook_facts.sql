alter table source_webhook_inbox
    add column event_kind text not null default 'push',
    add column head_repository_url text,
    add column head_repository_identity text,
    add column head_branch_name text,
    add column pull_request_id bigint
        check (pull_request_id is null or pull_request_id > 0),
    add column pull_request_number bigint
        check (pull_request_number is null or pull_request_number > 0),
    add column pull_request_change_kind text,
    add column pull_request_merged boolean,
    add column provider_created_at timestamptz,
    add column provider_updated_at timestamptz;

alter table source_webhook_inbox
    alter column event_kind drop default,
    add constraint source_webhook_inbox_event_kind_check
        check (event_kind in ('push', 'pull_request')),
    add constraint source_webhook_inbox_head_repository_check
        check (
            (
                head_repository_url is null
                and head_repository_identity is null
            )
            or (
                head_repository_url ~
                '^https://github[.]com/[a-z0-9]([a-z0-9-]{0,37}[a-z0-9])?/[a-z0-9._-]{1,100}$'
                and head_repository_identity =
                    'github:github.com/' || substring(head_repository_url from 20)
            )
        ),
    add constraint source_webhook_inbox_head_branch_check
        check (
            head_branch_name is null
            or (
                char_length(head_branch_name) between 1 and 255
                and head_branch_name !~ '^refs/'
                and head_branch_name !~ '^/'
                and head_branch_name !~ '/$'
                and head_branch_name !~ '[.]$'
                and head_branch_name !~ '[.][.]'
                and head_branch_name !~ '//'
                and head_branch_name <> '@'
                and head_branch_name !~ '(^|/)[.]'
                and head_branch_name !~ '[.](/|$)'
                and head_branch_name !~ '[.]lock(/|$)'
                and head_branch_name ~ '^[A-Za-z0-9._/-]+$'
            )
        ),
    add constraint source_webhook_inbox_typed_payload_check
        check (
            (
                event_kind = 'push'
                and head_repository_url is null
                and head_repository_identity is null
                and head_branch_name is null
                and pull_request_id is null
                and pull_request_number is null
                and pull_request_change_kind is null
                and pull_request_merged is null
                and provider_created_at is null
                and provider_updated_at is null
            )
            or (
                event_kind = 'pull_request'
                and head_branch_name is not null
                and pull_request_id is not null
                and pull_request_number is not null
                and pull_request_change_kind in (
                    'opened',
                    'synchronize',
                    'reopened',
                    'closed'
                )
                and pull_request_merged is not null
                and provider_created_at is not null
                and provider_updated_at is not null
                and provider_updated_at >= provider_created_at
                and (
                    pull_request_change_kind = 'closed'
                    or (
                        head_repository_url is not null
                        and pull_request_merged = false
                    )
                )
            )
        );

comment on column source_webhook_inbox.event_kind is
    'Authenticated provider event discriminator for the single Sources webhook inbox authority';

comment on table source_webhook_inbox is
    'Authenticated provider delivery evidence for push and pull-request facts; not a Preview aggregate, dispatch queue, retry rail, or provider payload store';
