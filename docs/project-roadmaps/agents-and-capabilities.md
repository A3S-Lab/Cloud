# Agent and Capability Project Roadmaps

These projects define what an Agent can do, how a host supervises it, and how
reviewed capabilities enter a running generation. They do not own Cloud
tenancy, placement, public routing, or node execution.

## A3S Code

**Mission:** provide the stateful Agent harness for Sessions, Runs, Turns,
subtasks, model calls, tools, skills, memory/context bindings, approvals, and
portable recovery boundaries.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `CODE-R1` | Finish one typed scope tree and immutable per-Run capability/model-input/tool-presentation evidence across built-ins and A3S Use generations | Child scopes cannot broaden authority; generation N continues on N while new Runs see N+1; cancellation settles every borrowed capability |
| `CODE-R2` | Stabilize Session/Run event journals, bounded immutable Tool output references, checkpoint export/import, exact recovery, fork inputs, and host persistence ports | Recovery binds exact catalog, authority ceiling, checkpoint components, object digest, and host revision without split visibility |
| `CODE-R3` | Add a generic Function invocation port supporting Cloud-hosted Task/Service functions and external FaaS connectors with typed request, deadline, cancellation, idempotency, and evidence | Agent tools call all modes through one contract; provider-specific credentials and retry rules never enter Code Core |
| `CODE-R4` | Complete Agent Service host protocol, readiness, drain, pause/resume, checkpoint handoff, streaming, health, and Runtime consumer conformance | A Cloud-managed warm Agent survives process/node replacement with no lost acknowledged event, duplicate side effect, or stale session owner |
| `CODE-R5` | Qualify multi-Agent delegation, shared Durable Cell bindings, Flow nodes, sessionless MCP tools, model-provider routing, hostile output handling, and observability | Every child, Cell operation, Flow run, Function call, and model call remains identity-, generation-, policy-, and trace-bound |

Code owns Agent semantics, not Cloud AgentRelease/Deployment, tenant IAM,
checkpoint authorization/retention, object storage, Runtime Unit lifecycle,
placement, autoscaling, public routes, Function releases, or provider secrets.

## Agent Harness Protocol

**Mission:** remain the transport-neutral, versioned supervision protocol
between an acting Agent and a policy/context harness.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `AHP-R1` | Freeze handshake, capability negotiation, event/decision taxonomy, request identity, ordering, batching, recursion bounds, errors, and compatibility | Golden protocol fixtures pass across maintained languages and transports; incompatible major versions fail closed |
| `AHP-R2` | Complete lifecycle, task, tool, approval, context, memory, rate, intent, verification, evidence-reference, cancellation, and backpressure contracts | Blocking controls cannot degrade into notifications; unavailable policy handlers return an explicit safe outcome |
| `AHP-R3` | Qualify stdio, local socket, HTTP, and streaming transports plus reconnect/replay guidance and security profiles | Transport changes do not alter decision semantics; duplicate and reordered messages are detected |
| `AHP-R4` | Publish conformance kits and adapters for A3S Code and external Agent frameworks | A non-Code Agent can be supervised without importing Cloud or Code internals |

AHP does not implement a harness, make authorization decisions, persist Agent
history, define a Runtime unit, or invent Cloud identity. It carries decisions
and evidence between separately owned components.

## A3S Use

**Mission:** own the trusted package graph and the atomic lifecycle that turns
reviewed packages into exact, scoped capability generations for arbitrary
Agent hosts.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `USE-R1` | Complete serializable package graph mutation, one Control Store, scoped installation authority, immutable plan, apply receipt, rollback, and crash recovery | Concurrent install/upgrade/remove selects one winner; lost responses replay; partial generations never publish |
| `USE-R2` | Stabilize Registry source enrollment, TUF trust roots/roles, signed catalog and target verification, dependency resolution, permissions, provenance, revocation, and offline behavior | Expired, rollback, freeze, mix-and-match, digest, length, path, and unauthorized-source attacks fail closed |
| `USE-R3` | Complete grants, bindings, capability snapshot cursors, non-clone leases, atomic host cutover, final-reader retirement, and reversible provider effects | A host acquires one dependency-closed generation; N remains alive for N readers and retires only after the final lease |
| `USE-R4` | Qualify managed MCP, Skill, Agent, Hook, Command, Flow, Knowledge, UI, Runtime Tool, and hardware capability providers through one lifecycle | Each provider declares permissions, health, cleanup, and exact evidence; no host-specific shadow registry remains |
| `USE-R5` | Operate the official Registry with signing ceremonies, delegated roles, mirrors, transparency/audit evidence, emergency revocation, and disaster recovery | Clean-host install and compromised-online-key drills preserve the offline trust root and recover safely |

Use does not own Cloud Organization/Project assignment, tenant billing,
workload placement, public routes, Runtime lifecycle, Agent sessions, or the
source code of installed providers. Cloud stores tenant enrollment and binding
intent; Use owns package truth and exact capability activation.

## A3S Use Packages

**Mission:** be the public, reviewable authoring source and signed publication
pipeline for the official A3S Use Registry.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `USEPKG-R1` | Freeze package source layout, manifest/schema validation, permission review, reproducible archive construction, and repository invariants | Pull requests cannot add links, private keys, unbound bytes, path escapes, duplicate identities, or undeclared permissions |
| `USEPKG-R2` | Perform the offline root-key ceremony and initialize signed root, targets, snapshot, and timestamp metadata with documented custody | Independent clients verify the bootstrap digest; test keys cannot enter production metadata |
| `USEPKG-R3` | Add delegated publisher roles, staged promotion, immutable targets, mirror publication, expiry monitoring, revocation, and incident drills | A compromised online role is revoked without replacing the offline root or accepting rollback metadata |
| `USEPKG-R4` | Publish reference packages for Browser, Office, OCR, Science, MHS, MCP, Skills, Agents, Flow nodes, and reviewed Runtime Tools | Every target installs through A3S Use, passes provider conformance, and uninstalls without residue |

Git is the review workflow, TUF metadata is the client trust authority, and A3S
Use is the installer. A package is never executed directly from a checkout,
and this repository does not become a tenant-specific Cloud Registry database.

## Integration exit

This group is ready when one exact Agent Run can:

- bind a signed, dependency-closed A3S Use capability generation;
- be supervised through AHP without framework-specific policy duplication;
- call a local or external Function through one typed Code port;
- invoke Flow, MCP, model, and Durable Cell capabilities with downward-only
  authority;
- export and restore one portable checkpoint through Cloud-owned immutable
  storage and fencing; and
- release every capability, process, credential view, and object lease after
  cancellation, replacement, or final Session close.

