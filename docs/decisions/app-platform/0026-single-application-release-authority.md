# 0026: Keep one immutable Application release authority

Status: Accepted

## Context

The six application experiences need product identity, delivery policy, and a
stable executable target. Workflow already owns graph and revision semantics;
A3S Flow already owns durable scheduling and history; Agents owns reasoning
events; and Workloads, Runtime, Box, Edge, Gateway, Identity, and Secrets own
their respective execution, traffic, and security state.

Putting a graph, provider loop, session store, route, or runtime controller in
each application mode would duplicate those authorities. Treating classic
Agent and New Agent as one mutable mode would also allow an existing
Application identity to silently change its capability and sandbox contract.

## Decision

Applications owns one `Application` aggregate and an immutable lineage of
`ApplicationRelease` records. An Application selects exactly one of six closed
experiences: Chatbot, Text Generator, classic Agent, New Agent, Chatflow, or
Workflow. The experience is immutable for that Application identity.

Every release uses canonical `cloud.application.release.v1` A3S ACL. It pins
one interaction mode, a bounded closed response-mode set, an audience policy,
and a presentation digest. It also binds one exact Workflow definition and
revision together with the Workflow contract, payload-set, semantic-contract-
set, input-schema, and output-schema digests. Publication admission must match
all of that evidence in the same organization and project.

Applications retains no Workflow graph, payload, mutable Workflow head, Plan,
run history, provider state, credential, session, or route in this contract.
Later persistence reuses the shared A3S ORM migration, idempotency, Outbox,
audit, and Resource Grant mechanisms. Later invocation calls typed owning
ports and the existing Workflow/Operation/Flow path.

## Consequences

All six experiences share one release and execution binding while preserving
the classic/New Agent distinction. A presentation change or a new Workflow
target creates another immutable release; changing experience requires a new
Application identity.

The component-only `APP0.1-C1/C2/C3` implementation adds strong identifiers,
the closed ACL contract, exact admission evidence, aggregate/release
invariants, and a checked-in conformance fixture. C2 adds only the Applications
head and immutable-release tables plus the A3S ORM transaction described by
decision 0027. C3 adds the authorization-before-replay CQRS and metadata-only
Workflow adapter described by decision 0028. It adds no public API,
session/message state, variable store, Flow workflow, queue, worker, provider
client, runtime, credential, object namespace, or Gateway route. Those
capabilities remain unavailable until their named APP0 gates pass.
