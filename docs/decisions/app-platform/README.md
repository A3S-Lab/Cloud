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

All six decisions are normative for `APP0`, `K0`, `AUT0`, and the remaining
`W0` application-platform work. A later change requires a superseding decision
and a new parity-manifest revision; it cannot silently reinterpret `v1`.
