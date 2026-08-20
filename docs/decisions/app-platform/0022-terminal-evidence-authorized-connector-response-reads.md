# 0022: Authorize Connector response reads through terminal evidence

Status: Accepted

## Context

Decision 0021 stores an accepted Connector response as one immutable object and
places only its exact reference, digest, and length in WorkflowRun version 6.
The object store can verify bytes, but object existence alone cannot authorize
their use: process death between the object write and C6 settlement may leave
an unreferenced object, and a structurally valid reference can still name the
wrong tenant, revision, attempt, digest, or length.

Typed node owners need a response-consumption boundary that preserves the C6
attempt authority and the shared immutable-object authority. A Flow step,
public download route, direct object client, or caller-selected storage key
would bypass one or both authorities and could place provider bodies in Flow,
API responses, logs, or another context's state.

## Decision

The existing `ConnectorExecutionApplicationService` implements the internal
`IConnectorResponseObjectPort`. A read request carries the exact
`cloud.connector.response-object.v1` reference and the current Resource Grant
projection. Connectors performs the checks in this order:

1. authorize the exact project and environment without disclosing a foreign
   object;
2. validate the derived attempt-scoped object reference;
3. load the exact organization, project, environment, profile, revision, and
   attempt record;
4. require accepted terminal C6 evidence and prove that the reference is the
   unique reference derived from that evidence; and
5. read through the existing Connector child of the shared immutable-object
   client and verify the digest and bounded length again.

The returned `ConnectorResponseObjectContent` is transient, non-serializable,
non-cloneable, and has a redacted `Debug` representation. It exposes bytes only
to an in-process typed consumer that already holds the port. The Workflow
coordinator, Flow history, Connector evidence rows, REST/OpenAPI, maintained
client, CLI, and Management MCP receive no body-read method.

## Consequences

An orphaned object without terminal evidence grants no authority. A denied
environment, missing attempt, nonterminal attempt, changed reference, missing
object, corrupt object, or unavailable store fails closed before bytes reach a
consumer. Exact repeat reads are deterministic and do not dispatch a provider
attempt.

This component-only `AUT0.5-C11` foundation adds no table, migration, public
route, object namespace, client, queue, worker, scheduler, retry counter,
provider call, or Flow schema. It does not make HTTP Request or `AUT0.5`
available: a typed owning node still has to consume the port, define its exact
body interpretation, and pass the remaining recovery and integration gates.
