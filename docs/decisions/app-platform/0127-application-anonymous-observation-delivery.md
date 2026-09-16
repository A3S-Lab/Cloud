# 0127. Application anonymous observation delivery

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.2-C54

## Context

APP0.2-C48/C49/C50 own anonymous-credential CQRS polls for Blocking, Streaming,
and Asynchronous observation. APP0.2-C53 admits anonymous sessions and
invocations on `/anonymous-delivery` with AUTH_PUBLIC_METADATA. Production still
needs one public presentation surface that polls those observations over HTTP
without inventing APP0.3 Gateway/SSE/browser, Identity secret verification, or a
second authorization authority based on application:write project membership.

Project-member observation delivery (APP0.2-C41/C44/C47) already uses
Principal-scoped polls under `/organizations/...`. Anonymous observation must
stay on the same public prefix as C53 and authorize only by opaque lookup key.

## Decision

Expose C48/C49/C50 through a public Applications presentation adapter only:

- REST GET under
  `/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/invocations/{invocation_id}/blocking-observation`,
  `.../streaming-observation`, and `.../asynchronous-observation`
- AUTH_PUBLIC_METADATA = true; do **not** attach OrganizationTenantGuard or
  AUTH_SCOPES_METADATA
- required query parameter `lookupKey` (opaque delivery credential lookup key)
- optional `afterSequence` on streaming observation (defaults to zero)
- server timestamp (`Utc::now()`) for `observed_at`
- OpenAPI contract 1.107.0
- maintained cloud client and CLI; optional application:write Management MCP tools may
  call the same C48/C49/C50 CQRS queries with an operator-supplied lookup key
  for ops testing (no second authority)
- reuse existing observation response DTOs

## Exclusions

- IssueApplicationDeliveryCredential / Secrets material / Environment invent
- SSE / Gateway / browser / embed
- anonymous close/cancel CQRS (APP0.2-C55 / ADR 0128) and public delivery (APP0.2-C56 / ADR 0129)

## Consequences

- Presentation adapters call the existing anonymous observation handlers only.
- Missing/inactive credentials and foreign sessions fail closed through CQRS.
- Live Gateway ingress and Identity-issued credential minting remain under
  APP0.3.

## Evidence

- OpenAPI contract version: 1.107.0
- Focused verification:
  cargo test -p a3s-cloud-control-plane --lib anonymous_blocking_observation
  cargo test -p a3s-cloud-control-plane --lib anonymous_streaming_observation
  cargo test -p a3s-cloud-control-plane --lib anonymous_asynchronous_observation
  cargo test -p a3s-cloud-control-plane --lib committed_openapi_snapshot
  cargo test -p a3s-cloud-control-plane --lib anonymous_
  CLI: application-anonymous-*-observation observe --lookup-key
