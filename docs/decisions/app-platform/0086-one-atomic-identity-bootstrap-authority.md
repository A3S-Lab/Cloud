# 0086: Bootstrap tenant identity and platform authority atomically

Status: Accepted

## Context

A fresh Cloud installation is unusable unless it has both a tenant identity
root and an Installation-scoped administrator root. Creating only an
Organization, Principal, owner Membership, and API token leaves no accepted
platform-role policy or `PlatformOwner` binding. Creating platform RBAC later
through another request leaves a partial-installation window and makes crash,
retry, and multi-replica behavior ambiguous.

The old bootstrap operation also lived on `IApiTokenRepository`, although it
coordinated several Identity aggregates. That made a credential repository the
owner of an installation-wide transaction and encouraged platform bootstrap to
become a second mechanism.

## Decision

`IIdentityBootstrapRepository` is the sole port for fresh-installation
authority. `IdentityBootstrap` is the validated result containing:

- one initial Organization;
- one active service Principal;
- one owner Membership and Principal-bound bootstrap API token; and
- one accepted baseline platform-role policy revision plus one active
  `PlatformOwner` binding for that same Principal.

The Application handler obtains the immutable database-owned Installation ID,
constructs the baseline A3S ACL policy and binding in the Domain, and submits
one `BootstrapIdentityWrite`. The public response remains the bounded
Organization and token projection; it does not expose policy internals.

The production implementation uses one PostgreSQL transaction. It:

1. validates the complete authority root;
2. acquires the existing bootstrap advisory transaction lock and then the
   canonical Installation mutation lock;
3. checks idempotency only after acquiring that lock, so identical concurrent
   requests deterministically become one commit and one replay;
4. inserts Organization, Principal, Membership, and API-token state;
5. calls the same transaction-local platform-RBAC bootstrap writer used by
   `IPlatformRbacRepository` to insert the accepted revision, current head,
   owner binding, shared Audit, and shared Outbox facts; and
6. stores one idempotent response before committing everything together.

The API-token repository no longer owns bootstrap. No second policy writer,
transaction coordinator, bootstrap table, Audit/Outbox rail, Redis/Lane lock,
cache truth, or distributed transaction is introduced. The in-memory adapter
mirrors the aggregate and events for component tests but is not production
evidence.

## Consequences

- A platform-fact failure rolls back every identity, credential, policy,
  binding, fact, and idempotency row. A retained PostgreSQL fault-injection
  gate proves this boundary.
- Concurrent and replayed public bootstrap must leave exactly one policy head,
  one revision, and one active matching `PlatformOwner` binding.
- Architecture tests prevent bootstrap from returning to
  `IApiTokenRepository` or acquiring another platform persistence mechanism.
- The code and local architecture/application/clippy gates passed on
  2026-08-29 in commit `459effcb`; main PostgreSQL 17 recertification is
  pending.
- This decision governs fresh installations. An installation created by an
  older build without a platform authority root still needs an explicit,
  operator-controlled recovery transition before the maintained administrator
  interfaces can be declared production-complete; public bootstrap must not
  silently adopt an existing tenant.
