create table source_webhook_inbox (
    provider text not null,
    delivery_id text not null,
    repository_url text not null,
    repository_identity text not null,
    installation_id bigint not null check (installation_id > 0),
    branch_name text not null,
    commit_sha text not null,
    payload_digest text not null,
    received_at timestamptz not null,
    primary key (provider, delivery_id),
    check (provider = 'github'),
    check (delivery_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    check (
        repository_url ~
        '^https://github[.]com/[a-z0-9]([a-z0-9-]{0,37}[a-z0-9])?/[a-z0-9._-]{1,100}$'
    ),
    check (
        repository_identity =
        'github:github.com/' || substring(repository_url from 20)
    ),
    check (
        char_length(branch_name) between 1 and 255
        and branch_name !~ '^refs/'
        and branch_name !~ '^/'
        and branch_name !~ '/$'
        and branch_name !~ '[.]$'
        and branch_name !~ '[.][.]'
        and branch_name !~ '//'
        and branch_name <> '@'
        and branch_name !~ '(^|/)[.]'
        and branch_name !~ '[.](/|$)'
        and branch_name !~ '[.]lock(/|$)'
        and branch_name ~ '^[A-Za-z0-9._/-]+$'
    ),
    check (
        commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'
        and commit_sha !~ '^0+$'
    ),
    check (payload_digest ~ '^sha256:[0-9a-f]{64}$')
);

create index source_webhook_inbox_repository_time_idx
    on source_webhook_inbox (
        provider,
        installation_id,
        repository_identity,
        received_at,
        delivery_id
    );
