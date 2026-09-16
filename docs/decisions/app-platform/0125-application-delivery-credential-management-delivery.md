# 0125. Application delivery credential management delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C52`

## Context

`APP0.2-C20` owns anonymous delivery credential register and generation-fenced
disable/enable/revoke. `APP0.2-C51` owns project-authorized get/list over the
same Applications repository. Management operators still need one REST/OpenAPI,
client, CLI, and Management MCP admission surface that reuses those handlers
without inventing a second credential authority, returning plaintext, or
coupling Environments into Secrets mint issuance.

`IssueApplicationDeliveryCredential` (`APP0.2-C30`) remains deferred from this
management delivery slice: exposing issuance would require a production Secrets
material port and Environment coupling that this gate must not invent.

## Decision

Expose C20 register/lifecycle and C51 get/list through the Applications
management boundary only:

- REST under application-scoped `/delivery-credentials` (collection register +
  list; item get; `disable` / `enable` / `revoke` lifecycle actions with
  `expectedGeneration`)
- OpenAPI contract `1.105.0`
- maintained cloud client, CLI, and `application:write` Management MCP tools
- responses carry opaque `lookupKey` plus exact Secret version reference only
  (never plaintext)
- server timestamps at mutation/admission time
- do **not** expose `IssueApplicationDeliveryCredential` yet

## Consequences

- Presentation adapters call the existing C20/C51 handlers only.
- Unauthorized projects and missing applications/credentials fail closed.
- Plaintext mint issuance stays C30-only until a later slice ports Secrets
  material without inventing Environment coupling.
- Public Identity-issued credentials remain under `APP0.3`.

## Evidence

- OpenAPI contract version: `1.105.0`
- Focused verification: `cargo test -p a3s-cloud-control-plane --lib modules::applications::application::delivery_credential`
