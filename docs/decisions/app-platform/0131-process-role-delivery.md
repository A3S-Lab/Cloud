# 0131. ProcessRole::Delivery capability boundary

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C2

## Context

APP0.3 requires a deployable `delivery` process that exposes published
application protocol surfaces without management CRUD or worker/relay
authority. `ProcessRole` previously admitted only `all`, `api`, `worker`, and
`relay`. Adding Gateway, SSE, browser/embed, or authenticated delivery HTTP in
the same slice would overfit before the role boundary exists.

## Decision

Admit `ProcessRole::Delivery` as a closed capability:

| Capability | All | Api | Delivery | Worker | Relay |
| --- | --- | --- | --- | --- | --- |
| `serves_management_api` | yes | yes | no | no | no |
| `serves_application_delivery` | yes | yes | yes | no | no |
| `runs_workers` | yes | no | no | yes | no |
| `runs_relay` | yes | no | no | no | yes |

- ACL parse accepts `role = "delivery"`; `all` may narrow to `delivery`
- Platform `/platform` reports `role: "delivery"`
- Delivery process-status boots until APP0.3-C3; C3 registers anonymous
  delivery HTTP without management routes (see ADR `0132`)

## Exclusions

- Authenticated delivery HTTP on the Delivery process (anonymous is APP0.3-C3)
- Gateway / SSE / browser / embed
- Identity-issued Principal-bound credential minting
- Rate limits, drain, rollback, routing recovery

## Consequences

- Operators can split `all` into `delivery` without widening other packaged
  roles.
- APP0.3-C3 owns registering published application delivery routes onto this
  role without importing management modules.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib process_roles_have_one_closed_capability_matrix`
- `cargo test -p a3s-cloud-control-plane --lib packaged_process_role_can_only_narrow`
- `cargo test -p a3s-cloud-control-plane --lib worker_relay_and_delivery`
- `cargo test -p a3s-cloud-control-plane --lib shipped_production_acl_is_one_valid`
