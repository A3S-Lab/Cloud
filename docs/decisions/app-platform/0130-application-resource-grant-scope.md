# 0130. Exact Application Resource Grant scope for delivery (Rule 8)

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C1

## Context

APP0.3 requires Identity-issued, Principal-bound authority that covers an exact
published Application. Project and Environment Resource Grants already exist for
restricted management navigation, but Project coverage of Application delivery
would violate Rule 8 (exact application authority). Delivery Role, Gateway,
SSE, browser/embed, and Environment-minted Secrets issuance remain later slices.

## Decision

Extend Identity Resource Grants with an exact Application scope:

- `ResourceGrantScope::Application { project_id, application_id }`
- `allows()`: Project covers Environment only; Application is exact and never
  implied by Project or Environment
- Applications ACL projection adds `ApplicationAccessScope::Application` with
  `exact_application_is_authorized` for delivery admission; Project still covers
  management visibility of applications in a project
- Migration `209` adds `application_id`, shape/kind checks, FK to
  `applications (organization_id, project_id, id)`, and unique active index
- CreateResourceGrant consults `IIdentityApplicationAccess` (Applications
  existence port) before write
- Transport: DTO, OpenAPI `1.109.0`, Management MCP schema, cloud client, CLI
- Token vocabulary: `application:invoke` constant reserved; not added to
  bootstrap scopes in this slice

## Exclusions

- `ProcessRole::Delivery`
- Gateway / SSE / browser / embed routes
- Identity-issued credential minting / Issue / Secrets Environment material
- Rate limits, drain, rollback, routing recovery

## Consequences

- Restricted memberships can be granted exact Application authority without
  project-wide delivery power.
- Later APP0.3 delivery admission can fail closed unless an exact Application
  grant (or organization-wide role) is present.
- Modules that do not own Application delivery discard Application grants in
  their ACL projections.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib resource_grant`
- `cargo test -p a3s-cloud-control-plane --lib resource_access_evaluator`
- `cargo test -p a3s-cloud-control-plane --lib modules::applications::application::resource_access`
- `cargo test -p a3s-cloud-control-plane --lib cloud_migration`
- `cargo test -p a3s-cloud-control-plane --lib resource_grant_application_scope_migration`
- `A3S_CLOUD_UPDATE_OPENAPI=1 cargo test -p a3s-cloud-control-plane --lib committed_openapi_snapshot`
