# Content and Application Provider Project Roadmaps

These projects supply reusable retrieval, memory, browser, document, science,
hardware, and interface capabilities. Their stable output is a typed provider
contract plus bounded evidence. A3S Use packages and activates them; A3S Code
or Cloud products consume them. They must not acquire a second copy of tenant,
deployment, or package authority.

## A3S Search

**Mission:** converge browser, HTTP/RSS, and native search sources into one
typed metasearch result boundary.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `SEARCH-R1` | Stabilize source contracts, canonical URLs, result identity, partial failures, deduplication, rank fusion, deadlines, cancellation, and bounded diagnostics | Deterministic fixtures cover duplicates, malformed sources, timeouts, rate limits, and partial success |
| `SEARCH-R2` | Add provider capability discovery, credential/egress ports, caching hooks, source health, adaptive concurrency inputs, and provenance evidence | Callers can reproduce which source produced each item without exposing credentials or private response bodies |
| `SEARCH-R3` | Package embedded and isolated providers through A3S Use and qualify Agent/Flow retrieval nodes | Provider replacement and cancellation leave no browser/process residue; exact package and policy generations are recorded |

Search is a retrieval kernel. It does not plan research, rewrite queries,
judge meaning, verify claims, write reports, or own Cloud search indexes.

## A3S Memory

**Mission:** provide minimal, pluggable memory storage and caller-owned
ephemeral vector-index primitives.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `MEMORY-R1` | Freeze MemoryStore identity, item revision, type, metadata, deterministic scoring, deduplication, pruning, and atomic persistence semantics | Store conformance covers replay, concurrent update, corruption, restart, pinned retention, and bounded queries |
| `MEMORY-R2` | Add replaceable durable and vector providers, export/import, encryption hooks, retention evidence, metrics, and migration contracts | Provider swaps preserve item identity and declared recall behavior without converting the vector index into truth |
| `MEMORY-R3` | Qualify Code Session/Run bindings and Cloud-managed storage adapters through narrow ports | Agent context assembly records exact memory revision while Cloud controls tenant authorization and retention |

Memory does not own Agent session policy, context injection, embeddings,
Knowledge admission, semantic truth, tenant lifecycle, or a distributed vector
service control plane.

## A3S Browser

**Mission:** provide one embedded page-rendering contract and one deliberately
separate process-isolated browser automation driver.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `BROWSER-R1` | Stabilize PageRenderer requests/results, lifecycle, session identity, capability discovery, screenshots/content references, deadlines, cancellation, and bounded output | Chrome-compatible and optional providers pass navigation, crash, cancellation, hostile-page, download, and cleanup tests |
| `BROWSER-R2` | Harden the driver CLI/MCP/Skill automation surface, process protocol, accessibility/vision grounding, download/upload, network policy, and secret isolation | Tool schemas and actions are deterministic; browser profiles cannot cross tenant/session or survive final close unexpectedly |
| `BROWSER-R3` | Publish signed A3S Use packages and conformance for Search, Code, Flow, and Test consumers | Embedded Search never imports the driver; managed automation never leaks its mutable state into library callers |

Browser does not own Cloud routes, tenancy, research planning, website trust,
or Agent authorization. Any bundled local dashboard belongs to the Browser
provider and is not an A3S Cloud management dashboard.

## A3S OCR

**Mission:** convert bounded image batches into layout-aware text and geometry
using qualified CPU/GPU providers.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `OCR-R1` | Complete provider/evidence foundation, staged batches, orientation, detection, recognition width buckets, and deterministic preprocessing | Golden visual/text fixtures bind model, weights, preprocessing, device, and output geometry |
| `OCR-R2` | Keep OCR stages device-resident where supported, add bounded batching, cancellation, fallback declarations, language/model breadth, and resource telemetry | CPU/GPU parity, memory pressure, oversized input, corrupt image, partial batch, and cleanup suites pass |
| `OCR-R3` | Integrate Parser, Office, Code, and A3S Use packages with exact revision and model-supply bindings | Consumers receive typed evidence and never depend on an implicit global OCR model or mutable cache |

OCR does not own document lifecycle, model catalog, weight licensing, GPU
placement, tenant files, or semantic document interpretation.

## A3S Parser

**Mission:** turn Office, PDF, image, and related formats into one structured,
cross-page document representation with explicit fidelity profiles.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `PARSER-R1` | Freeze document, page, block, table, image, style, relation, warning, and source-evidence contracts plus deterministic profile selection | Canonical fixtures reject corrupt/hostile archives, path escapes, oversized structures, and unsupported features safely |
| `PARSER-R2` | Complete OOXML Fast, Office visual bridge, Balanced/Deep reconciliation, PDF/image breadth, OCR integration, and streaming batches | Cross-page order, tables, notes, formulas, embedded media, OCR regions, and partial recovery pass fixture suites |
| `PARSER-R3` | Add scale, durable intermediate references, cancellation, observability, A3S Use packaging, and Code/Flow/Knowledge ports | Large-document processing resumes without duplicating committed stages and cleans all intermediates |

Parser does not own source files, Knowledge admission, OCR/model deployment,
editor persistence, or tenant workflow state.

## A3S Office

**Mission:** provide embeddable, format-native browser editors and deterministic
native automation while the host retains product state and policy.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `OFFICE-R1` | Complete document, Markdown, spreadsheet, presentation, and PDF editing parity with typed Core, React, Vue, and Web Component host boundaries | Visual, accessibility, import/export, undo/redo, keyboard, mobile, and framework conformance pass |
| `OFFICE-R2` | Harden native CLI/MCP/Skill automation, immutable operation evidence, cancellation, file safety, and cross-format conversion | Known operations are deterministic, bounded, replay-safe where declared, and leave no partial overwrite |
| `OFFICE-R3` | Define an optional collaboration port that can bind A3S Durable Cell without embedding Cloud identity or storage | Multi-user presence/edits recover through host-supplied revisions; standalone Office remains usable without Cloud |
| `OFFICE-R4` | Publish signed A3S Use packages, static Web release examples, and Agent/Flow nodes | Package install and UI delivery preserve exact Office, asset, permission, and host contract digests |

Office does not own tenant identity, authorization, persistence, Durable Cell
deployment, model routing, or A3S Cloud's management surface.

## A3S Science

**Mission:** curate a reviewable scientific capability atlas and publish
trusted Skills, MCP servers, agents, workbenches, and metadata packages.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `SCIENCE-R1` | Stabilize bilingual taxonomy, resource identity, provenance, classification, capability metadata, source links, and validation | Catalog changes are reviewable, duplicate-free, link-checked, and reproducible |
| `SCIENCE-R2` | Publish signed A3S Use targets for native resources and managed entry contracts without redistributing unapproved upstream binaries | Every target binds permissions, archive bytes, source, license, and expanded-content digest |
| `SCIENCE-R3` | Qualify high-value literature, computing, modeling, visualization, and reproducibility workflows with Bench evidence | A clean environment can install, execute, attribute, and remove each promoted reference capability |

Science does not become a second Use Registry protocol, Cloud model catalog,
or general package installer. Its signed publication follows the common A3S
Use trust contract.

## A3S MHS

**Mission:** define a replaceable material-handling-system simulation and real
hardware capability boundary for Agents and workflows.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `MHS-R1` | Complete deterministic simulation, scenario/state/action contracts, browser workbench, and replayable evidence | Fixed seeds and inputs reproduce outcomes; invalid physical transitions fail before actuation |
| `MHS-R2` | Add industrial scenarios, replaceable physics/robotics providers, safety interlocks, digital-twin correlation, and bounded telemetry | Fault injection, emergency stop, stale command, sensor loss, and simulation/real divergence tests pass |
| `MHS-R3` | Qualify the real hardware boundary as an A3S Use capability with explicit grants, human approval, leases, and cleanup | No Agent can bypass Use generation, Sentry policy, hardware fencing, or operator stop authority |

MHS does not own A3S Use installation, Cloud scheduling, generic Workflow
history, or system-wide authorization. Physical safety remains fail closed and
separate from model judgment.

## A3S UI

**Mission:** provide one framework-neutral design system and interaction
language for A3S product surfaces and tenant-owned Agent applications.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `UI-R1` | Stabilize tokens, semantic components, application shells, agent workbenches, document tools, accessibility, density, themes, and internationalization | Visual, keyboard, screen-reader, responsive, reduced-motion, and browser matrices pass |
| `UI-R2` | Publish versioned CSS/runtime bundles, framework adapters, composition guidance, and static-hosting examples | React/Vue/framework-neutral examples build reproducibly into immutable Web releases |
| `UI-R3` | Add Agent/Flow/approval/trace/log/usage primitives for client applications without embedding backend authority | Components consume typed client state and cannot authorize, schedule, or mutate Cloud state independently |

A3S UI may be used by tenant applications and local clients. This roadmap does
not introduce an A3S Cloud Dashboard; Cloud management remains API, OpenAPI,
maintained clients, CLI, and Management MCP.

## A3S Form

**Mission:** provide small, accessible, schema-bound form rendering and value
editing primitives shared by UI, Flow authoring, and capability packages.

The current workspace has no initialized implementation contract, so the first
milestone is deliberately a charter rather than speculative code.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `FORM-R0` | Decide repository scope, value/schema contract, supported controls, dependency direction with A3S UI, and whether a separate package is justified | Accepted ADR proves the work cannot be expressed as an A3S UI module without creating duplicate ownership |
| `FORM-R1` | If retained, implement deterministic field rendering, validation messages, conditional visibility, arrays/objects, accessibility, and framework-neutral events | Cross-framework fixtures produce equivalent values and errors; hostile/oversized schemas remain bounded |
| `FORM-R2` | Add Flow node/property editors and capability configuration projections using host-supplied schemas | Form never parses product configuration or bypasses owning API validation; submitted values are revalidated by the owner |

Form must be merged into A3S UI if `FORM-R0` cannot demonstrate an independent
lifecycle and public contract. It never becomes a product configuration
language, workflow engine, or policy evaluator.

## Integration exit

This group is ready when every promoted capability:

- has one typed contract, immutable package/revision identity, declared
  permissions, bounded inputs/outputs, cancellation, health, and cleanup;
- is installed through A3S Use rather than loaded from a mutable checkout;
- exposes evidence sufficient for Code, Flow, Cloud, Test, and Bench without
  leaking secrets or provider internals; and
- keeps product state, tenant policy, placement, models/weights, and public
  routing with their named owners.

