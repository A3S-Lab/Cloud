# 0097. Application message-variant authority

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C25`

## Context

The platform model already names idempotent `ApplicationMessageVariant`
("More Like This") beside sessions and messages, but Applications had no domain
authority for it. Remaining APP0.2 work still listed message variants after
C24. Persistence, CQRS, regeneration invocation, and public delivery must not
invent a second conversation history or rewrite ordered messages.

## Decision

`APP0.2-C25` freezes `ApplicationMessageVariant` as an immutable
Applications-owned record bound to an exact organization/project/application/
release digest, session, and end user. The required source message must belong
to that exact session and must be `Answer` or `FinalOutput`. The variant stores
the source message's `invocation_id` so the exact input remains linked. Optional
instruction JSON is bounded and digested. Deterministic UUIDv5 identity derives
from the session, source message, invocation, and instruction digest so exact
create replays without rewriting ordered `ApplicationMessage` sequences.
Inactive sessions, Input sources, foreign source messages, oversized
instructions, and identity drift fail closed. One in-memory conformance path
proves the rules.

## Consequences

`APP0.2-C25` remains component-only. No migration, repository, CQRS command,
regeneration invocation, REST/MCP, Gateway, SSE, or public availability is
added. Persistence and write commands stay later numbered gates.
