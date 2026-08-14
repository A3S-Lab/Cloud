# AI Application Platform Decisions

These accepted decisions define the authority boundaries used by the versioned
AI application platform parity manifest.

| Decision | Outcome |
| --- | --- |
| [0001](0001-flow-preservation.md) | Preserve A3S Flow as the sole durable orchestration authority |
| [0002](0002-application-delivery.md) | Deliver all application modes through one immutable release and invocation path |
| [0003](0003-step-descriptor-registry.md) | Add immutable Cloud-owned semantic descriptors without widening Flow into a product graph |
| [0004](0004-trigger-authority.md) | Create new invocations only through Automations |
| [0005](0005-file-authority.md) | Separate user-file metadata from shared immutable bytes and runtime working files |
| [0006](0006-knowledge-authority.md) | Keep Knowledge corpus truth separate from Workflow ontology and Search indexes |
| [0007](0007-typed-variable-scopes.md) | Freeze Workflow-owned typed scopes without adding a second mutable state engine |
| [0008](0008-revision-bound-plan-v2.md) | Persist revision-owned semantic contracts and pin exact execution semantics in Plan v2 |
| [0009](0009-workflow-node-catalog-projection.md) | Compose read-only built-in node discovery without creating another descriptor or execution authority |
| [0010](0010-flow-derived-variable-inspection.md) | Inspect runtime variables by replaying immutable input and the sole A3S Flow history |
| [0011](0011-digest-bound-variable-defaults.md) | Materialize digest-bound defaults from immutable WorkflowRevision and Run input without a variable store |

All eleven decisions are normative for `APP0`, `K0`, `AUT0`, and the remaining
`W0` application-platform work. A later change requires a superseding decision
and a new parity-manifest revision; it cannot silently reinterpret `v1`.
