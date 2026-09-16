# 0124. Application delivery credential get/list CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C51`

## Context

`APP0.2-C20` and `APP0.2-C30` own anonymous delivery credential register,
generation-fenced lifecycle, and Secrets mint issuance. Operators still need
one project-authorized read path over the same Applications repository before
any management REST surface can reuse it. Inventing a second credential catalog
or public delivery route would reopen APP0.3 ownership.

## Decision

`APP0.2-C51` adds project-authorized Applications queries that:

1. Authorize the Project before any read.
2. Get one credential by exact Application-scoped identity through the existing
   C17 repository, failing closed for missing or foreign bindings.
3. List credentials for one exact Application after proving the Application
   exists in the same Organization/Project, ordered by created_at then id.
4. Return the Applications-owned binding only (opaque lookup key and Secret
   version reference; never plaintext).

No migration, REST/OpenAPI/client/CLI/MCP, Gateway, SSE, or public availability
is added.

## Consequences

- Management delivery can later reuse these queries without a second read model.
- Unauthorized projects and missing applications/credentials fail closed.
- Plaintext issuance remains C30-only; public Identity-issued credentials stay
  under `APP0.3`.
