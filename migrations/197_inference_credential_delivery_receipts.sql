create table inference_credential_delivery_receipts (
    credential_id uuid primary key,
    organization_id uuid not null,
    generation bigint not null
        check (generation between 1 and 9007199254740991),
    key_id text not null
        check (octet_length(key_id) between 1 and 512),
    ciphertext text not null
        check (octet_length(ciphertext) between 1 and 2097152),
    expires_at timestamptz not null,
    created_at timestamptz not null,
    foreign key (organization_id, credential_id)
        references inference_credentials (organization_id, id)
        on delete cascade,
    check (expires_at > created_at)
);

create index inference_credential_delivery_receipts_expiry_idx
    on inference_credential_delivery_receipts (expires_at, credential_id);

comment on table inference_credential_delivery_receipts is
    'Bounded encrypted recovery receipts for idempotent Identity inference-key delivery; never stores plaintext bearer values';
