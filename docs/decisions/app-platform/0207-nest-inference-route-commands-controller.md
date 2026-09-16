# 0207. Nest-macro inference route commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Inference route publish/revise/retire are the command siblings of Nest
query ADR `0194`.

## Decision

Convert `inference_route_commands_controller` to Nest macros with
`OrganizationTenantGuard`, `INFERENCE_WRITE` scopes metadata, and `raw` POST
handlers. Keep the factory for module registration. Refuse inventing `S0` BYOK
and refuse marking Nest migrations as full production release.

## Consequences

Completes Nest coverage for the inference-route HTTP pair. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
