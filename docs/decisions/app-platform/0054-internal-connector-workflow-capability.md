# 0054: Advertise the exact Connector Workflow step as internal

Status: Accepted

## Context

Decisions 0020 through 0025 established the component-only business-service
path: an exact immutable Connector revision is dispatched through the
Connectors-owned application port, A3S Flow owns bounded observation and retry
waits, accepted bytes are admitted to immutable object storage, and one
schema-bound JSON value or descriptor-bound failure is projected into Workflow
history. The application-platform manifest still classified HTTP Request as
unavailable, which incorrectly described an implemented internal capability as
an absent implementation.

`internal` is deliberately distinct from public product availability. It means
that an owning implementation slice exists and can be represented accurately by
the read-only node catalog; it does not verify the remaining `AUT0.5`, `W0.4`, or
`W0.5` gates.

## Decision

`node.http-request` is an internal, Connectors-owned `service` capability with
the exact `connector.http` semantic profile. Its evidence binds the existing
Flow coordinator, response projection, immutable-object read boundary, failure
route, and focused tests.

Execution still requires one exact non-nil `ConnectorRevision` identity and
digest, one authorized Environment, and the bounded Workflow provider policy.
The Connector profile remains the sole authority for method, destination,
schema, egress, and Secret references. A Workflow cannot persist an arbitrary
URL, injected header environment, plaintext Secret, provider fence, or response
body. Catalog presence does not admit a descriptor; the immutable descriptor
registry bound to the Workflow revision remains the compilation authority.

Public HTTP Request availability remains closed until the outstanding Connector
provider and event-consumer wiring, revocation and recovery operations, retained
end-to-end evidence, and the applicable `AUT0.5`, `W0.4`, and `W0.5` gates pass.
No REST/OpenAPI version changes because the existing catalog schema already
supports the `internal` state and the response shape is unchanged.

## Consequences

The catalog now distinguishes the implemented business-service foundation from
the capability steps that have no admitted runtime. It still makes no public or
production claim. This decision adds no table, migration, queue, scheduler,
retry authority, HTTP client, object client, public endpoint, or Flow schema.
