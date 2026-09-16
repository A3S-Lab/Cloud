# 0128. Application anonymous session close and invocation cancel CQRS

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.2-C55

## Context

APP0.2-C18/C19 admit anonymous sessions and invocations over opaque delivery
credential lookup keys. APP0.2-C48/C49/C50 observe them. APP0.2-C53/C54 expose
admission and observation on `/anonymous-delivery`. Project-member close and
cancel already exist as durable CQRS (`CloseApplicationSession`,
`CancelApplicationInvocation`). Production still needs the same lifecycle stop
paths for anonymous-credential sessions without inventing Gateway/SSE, Issue /
Secrets Environment minting, or a second `application:write` authority.

ADR 0126/0127 explicitly deferred anonymous close/cancel until CQRS exists.

## Decision

Own anonymous close and cancel as Applications CQRS only:

- `CloseAnonymousApplicationSession` — opaque `credential_lookup_key` +
  session identity/version; reuses member close durability and
  `CloseApplicationSessionResult`
- `CancelAnonymousApplicationInvocation` — opaque lookup key +
  session/invocation identity/version; reuses member cancel durability,
  WorkflowRun cancellation port, and `CancelApplicationInvocationResult`
- authorize via `load_credential_by_lookup_key` +
  `anonymous_credential_session` + anonymous release/end-user checks
- **do not** re-require Active credential status for already-admitted
  sessions/invocations (disable mid-flight still allows shutdown/cancel);
  foreign lookup keys fail closed
- register handlers in control-plane composition; no public HTTP in this gate

## Exclusions

- Public `/anonymous-delivery` close/cancel presentation (APP0.2-C56 / ADR 0129)
- IssueApplicationDeliveryCredential / Secrets material / Environment invent
- Gateway / SSE / browser / embed
- Project-member close/cancel behavior changes

## Consequences

- Anonymous lifecycle stop is Applications-owned and testable without HTTP.
- Presentation adapters can later call these handlers only.
- Identity-issued minting and live Gateway ingress remain under APP0.3.

## Evidence

- Focused verification:
  `cargo test -p a3s-cloud-control-plane --lib anonymous_lifecycle`
