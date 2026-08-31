# Foundations and Execution Project Roadmaps

This document plans the reusable language, application, persistence, event,
and execution mechanisms below A3S Cloud. These projects must remain product
neutral: Cloud composes them through ports and published contracts rather than
moving tenant or product semantics into infrastructure libraries.

## A3S ACL

**Mission:** remain the only product configuration language in A3S and produce
one canonical, diagnosable, digest-bound representation for every product
specification.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `ACL-R1` | Freeze parser, formatter, source spans, canonical ordering, schema validation, and domain-separated digest contracts | Golden fixtures round-trip byte-stably; malformed, ambiguous, duplicate, oversized, and unknown fields fail with bounded diagnostics |
| `ACL-R2` | Provide versioned schema registration, compatibility checks, migrations, lint rules, and typed code generation without compatibility parsers | Old supported revisions migrate deterministically; unsupported revisions fail closed; generated types match fixtures in all maintained SDKs |
| `ACL-R3` | Add streaming/size bounds, fuzzing, corpus minimization, supply-chain signing hooks, and conformance kits for Cloud, Runtime, Flow, Gateway, and Use | No consumer implements a private parser or hashes non-canonical input; fuzz, denial-of-service, and cross-language digest suites pass |

ACL does not decide tenant policy, authorization, placement, retry, or business
defaults. A bounded context owns those decisions and supplies the schema.

## A3S ORM

**Mission:** provide executor-neutral, type-safe SQL construction and the local
transaction primitives required by repositories without becoming an Active
Record domain layer.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `ORM-R1` | Stabilize typed queries, parameters, row decoding, migration primitives, transaction scopes, savepoints, and backend capability discovery | PostgreSQL conformance and compile-fail suites reject unsafe identifiers, binding drift, partial decoding, and unsupported features |
| `ORM-R2` | Add reusable optimistic-concurrency, compare-and-swap, idempotency-record, lease/fence, keyset pagination, advisory-lock, and transactional-outbox building blocks | Concurrent writers prove one winner; lost responses replay; fence tokens reject stale owners; all helpers remain table- and domain-neutral |
| `ORM-R3` | Add statement observability, bounded retry classification, pool pressure signals, migration locks, schema-drift checks, and backup/restore qualification hooks | Callers can correlate a transaction without logging secrets; retry never crosses an unknown commit outcome automatically |

ORM does not own business repositories, aggregate boundaries, cross-provider
transactions, saga policy, or distributed XA. The Application layer chooses
the transaction boundary; Cloud operations and Flow coordinate work outside
one database commit.

## A3S Boot

**Mission:** provide adapter-first modular process composition and explicit,
testable cross-cutting pipelines for Rust services.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `BOOT-R1` | Complete module encapsulation, provider scopes, dependency-cycle diagnostics, lifecycle hooks, CQRS dispatch, and deterministic startup/shutdown | Private providers cannot escape a module; startup failure unwinds in reverse order; command/query handlers have one registration |
| `BOOT-R2` | Complete HTTP extraction, validation, OpenAPI metadata, middleware, guards, interceptors, pipes, exception filters, WebSocket, and supported service transports | Aspect order is explicit and invariant tests cover success, rejection, panic, cancellation, and streaming termination |
| `BOOT-R3` | Add dynamic modules, health/readiness composition, graceful drain, configuration snapshot injection, telemetry hooks, and test harnesses | One composition root can build each Cloud process role with no service locator in Domain or Application code |

Boot owns aspect mechanics, not aspect policy. Cloud owns authentication,
authorization, idempotency, rate-limit, cache, transaction, audit, and
redaction decisions and registers those policies explicitly.

## A3S Event

**Mission:** provide pluggable event envelopes, subscriptions, dispatch, and
transport/storage adapters while preserving producer-owned semantics.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `EVENT-R1` | Freeze envelope identity, type/revision, producer, occurred time, trace correlation, content digest, ordering key, and bounded metadata | Canonical fixtures and compatibility tests pass across in-memory and NATS providers |
| `EVENT-R2` | Qualify consumer groups, acknowledgements, redelivery, deduplication hooks, backpressure, encryption, dead-letter transport, and replay cursors | Broker restart, lost acknowledgement, poison message, consumer replacement, and retention-boundary tests pass |
| `EVENT-R3` | Publish provider health, lag, delivery, and reconciliation evidence with operational tooling | Cloud can rebuild consumers from the owner outbox without treating the broker as business truth |

Event does not decide which aggregate transitions publish, create a second
Outbox, retry non-idempotent business work, or own workflow history. Domain
events are committed by their bounded context; Event transports committed
facts.

## A3S OCI Runtime

**Mission:** own the complete low-level execution and isolation lifecycle for
one OCI process, container, or utility-VM boundary behind a stable local SDK
and service protocol.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `OCI-R1` | Complete journaled create/state/start/kill/delete/wait/exec, exact generation identity, reconnect, response-loss recovery, and leak quarantine | Native Linux plus selected MicroVM drivers pass crash/reopen/replay/cleanup conformance |
| `OCI-R2` | Complete immutable bundle validation, namespaces, cgroups, seccomp, mounts, devices, network attachments, signals, PTY, pause/resume, stats, and checkpoint primitives | Unsupported isolation combinations fail before launch and no driver silently weakens isolation |
| `OCI-R3` | Qualify KVM, HVF, WHPX, and native Linux promotion separately, including guest authentication, attestation, upgrades, and containerd runtime-v2 integration | Each advertised platform has hardware-backed evidence, signed artifacts, clean-host installation, and rollback limits |

OCI Runtime does not pull or build images, manage registries, allocate product
networks or volumes, schedule cluster placement, or understand Agent,
Workflow, Function, Cell, MCP, or tenant semantics.

## A3S Box

**Mission:** be the node-local product engine that compiles generic workload
requirements into images, root filesystems, networks, volumes, secrets, and
one exact OCI Runtime generation.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `BOX-R1` | Finish the one-way OCI Runtime migration and remove parallel execution paths after compatibility evidence | Every advertised Sandbox/MicroVM path uses the pinned SDK; legacy direct drivers are absent |
| `BOX-R2` | Qualify pull/push/build/sign/verify/cache, network/IPAM/DNS, named volumes/snapshots, secret attachment, logs, health, resources, and outputs | Real private-registry, mount, network, resource, secret, restart, and inventory-equality suites pass |
| `BOX-R3` | Expose one versioned Runtime provider capability matrix, deterministic rejection reasons, endpoint observations, recovery, and cleanup evidence | Runtime accepts only advertised features and stale generations cannot retain endpoints or resources |
| `BOX-R4` | Add node-pressure, image/weight cache accounting, drain, warm-pool primitives, and upgrade safety without cluster policy | Cloud can make placement decisions from fresh capacity evidence; Box never chooses another node or desired replica count |

The `BOX-R3` identity-evidence and Service-lifecycle sub-slices are verified at
`36086bd1b8ddafa6d1228251cefa55dacbbaee5c`: only the confidential provider
advertises `IdentityAttachment`, restart/replay preserves the attachment in
provider evidence and attestation, and readiness, liveness-triggered restart,
graceful shutdown, and cleanup retain exact generation evidence. The exact
[main CI](https://github.com/A3S-Lab/Box/actions/runs/33393067843) passes native
and aarch64 OCI lifecycle plus all four SDKs. This does not complete Box
networking, pressure, hardware-provider, recovery, or upgrade gates.

Box does not own Cloud Workloads, Fleet placement, autoscaling, tenant quotas,
public routes, Runtime Unit identity, product registries, or AI product
semantics.

## A3S Runtime

**Mission:** provide the provider-neutral, durable lifecycle contract for one
generic `Task` or `Service` unit over A3S Box.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `RUNTIME-R1` | Land the unified consumer-requirements contract for Agent Service, Function Task, Function Service, sessionless MCP Service, Durable Cell Service, and generic workloads | All profiles use `Task` or `Service`, generic features, opaque semantics digests, health, and endpoint requirements; no product-specific Unit class exists |
| `RUNTIME-R2` | Complete apply/inspect/stop/remove/logs/exec, immutable spec identity, request replay, typed endpoint observations, health, outputs, and provider fencing | Lost response, agent loss, provider loss, restart, replacement, stale observation, and cleanup conformance pass over real Box |
| `RUNTIME-R3` | Add generic outbound-policy attachment, pause/resume, checkpoint/restore reference, resource-update evaluation, and capability discovery only where a provider can prove them | Every optional feature is independently advertised, digest-bound, generation-bound, and rejected when unsupported |
| `RUNTIME-R4` | Publish stable protocol/SDK compatibility, mixed-version recovery, telemetry, capacity feedback, and exact-revision consumer fixtures | Cloud can upgrade Runtime agents without two owners, endpoint ambiguity, or orphaned resources |

The identity-attestation and Service-lifecycle parts of `RUNTIME-R1/R3` are
implemented in Runtime `0.5.0` at
`4c5fbd56bedd84d1007a7d9cd046a9f7083bbdcd`. One opaque attachment is
validated across Unit Spec and evidence; `RuntimeAttestationBinding` closes
Unit/generation/Spec/provider resource/build/attestation identity without
interpreting product policy. The same generic Service contract now carries an
optional liveness probe and bounded graceful-shutdown interval without adding
a product-specific Unit class. Cloud and the pinned Box integration verify
that contract; Runtime host and hardware promotion remain separate gates.
Cloud, not Runtime, owns freshness, tenant policy and issuance decisions.

Runtime does not own Agent sessions, Function invocations, Workflow history,
Durable Cell records, MCP protocol, model routing, placement, replicas,
autoscaling, routes, tenancy, billing, or credentials. Those products bind an
opaque semantics profile to the same generic lifecycle.

## Integration exit

This group is ready for the next portfolio wave only when:

- ACL fixtures compile to the same digests consumed by Boot-composed services;
- Cloud repositories use ORM primitives without leaking ORM models into Domain;
- Domain event and Outbox commits are atomic while Event delivery remains
  replayable and replaceable;
- Box has exactly one low-level execution path through OCI Runtime;
- Runtime has exactly two Unit classes and all AI product profiles consume
  them through the same requirements contract; and
- exact Runtime, Box, OCI Runtime, ACL, and fixture revisions pass real-host
  failure and cleanup tests.
