# Operations, Client, and Release Project Roadmaps

This group makes execution observable, enforceable, usable, testable, and
installable. Client and telemetry projects remain consumers of platform truth;
they do not become shadow control planes.

## A3S Observer

**Mission:** acquire kernel-level process, file, network, security, and model-
call evidence for workloads without requiring per-language instrumentation,
and expose opt-in enforcement primitives.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `OBSERVER-R1` | Stabilize event identity, process/workload correlation, loss counters, bounded/redacted argv and content, clock calibration, and OpenTelemetry export | PID reuse, fork/exec, truncated input, ring-buffer loss, restart, and identity-drift fixtures are explicit |
| `OBSERVER-R2` | Qualify egress, file, and execution guards separately from passive observation with atomic policy generation changes | Observe remains available when enforcement policy fails; stale policy cannot override a newer fence |
| `OBSERVER-R3` | Bind Runtime Unit, Cloud Workload, Agent Run, Function Invocation, WorkflowRun, inference request, node, tenant, and trace identities | Operators can traverse a request to kernel actions without inferring tenant identity from mutable process names |
| `OBSERVER-R4` | Add multi-node deployment, overhead budgets, upgrade, self-observation, security hardening, and clean uninstall | Long-running performance and failure tests meet declared overhead and evidence-loss budgets |

Observer does not decide IAM, business authorization, desired state, or
security judgment. It observes and enforces an explicitly supplied policy
generation.

## A3S Sentry

**Mission:** judge runtime-security evidence through deterministic and
escalating policies, then request enforcement through Observer's single kernel
mechanism.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `SENTRY-R1` | Freeze event normalization, deterministic rule evaluation, precedence, deny/escalate/allow outcomes, evidence, and fail-closed behavior | Ambiguous/truncated evidence escalates or denies according to signed policy; no exception becomes implicit allow |
| `SENTRY-R2` | Qualify bounded LLM and deep-Agent escalation, deadlines, budgets, cancellation, prompt-injection isolation, and human approval | Unavailable or compromised higher tiers cannot weaken a deterministic lower-tier deny |
| `SENTRY-R3` | Add signed policy generations, staged rollout, simulation, rollback, incident correlation, and Cloud tenant/system policy adapters | Every enforcement decision binds exact policy, evidence, workload, principal, and fence generation |
| `SENTRY-R4` | Qualify optional Power activation evidence and confidential-processing paths separately | Unsupported attestation or activation access fails closed and does not change baseline policy semantics |

Sentry is the runtime-security policy brain, not Cloud IAM, Gateway request
authorization, a general moderation service, or the kernel enforcer.

## A3S CLI

**Mission:** be the canonical command client for local A3S development and all
supported remote Cloud management operations.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `CLI-R1` | Keep one component lifecycle for install, inspect, upgrade, repair, rollback, and uninstall with exact version/digest evidence | Clean-host and interrupted-upgrade matrices pass across supported platforms |
| `CLI-R2` | Generate or maintain Cloud commands from public OpenAPI/client contracts for tenants and system administrators | CLI sends the same idempotency, precondition, pagination, operation, and error semantics as maintained SDKs |
| `CLI-R3` | Integrate Code, Use, Box, Flow, Test, Bench, logs, traces, and local development without copying their domain logic | Commands delegate to owner APIs/libraries and expose exact owner receipts |
| `CLI-R4` | Add profiles, secure credential-store adapters, device/login flows, context switching, completion, automation-safe output, and offline diagnostics | Secrets never enter process arguments or logs; machine output is versioned and backward-compatible |

CLI does not contain hidden scheduler, authorization, registry, retry, or
reconciliation logic. It remains a replaceable client, not a Cloud replica.

## A3S TUI

**Mission:** provide a reusable Elm-architecture terminal interface framework
with deterministic state, layout, input, and rendering.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `TUI-R1` | Stabilize Model/Update/View, effects, Flexbox layout, incremental rendering, Unicode width, focus, input, and component contracts | Golden terminal frames, property tests, resize, paste, mouse, accessibility, and low-color matrices pass |
| `TUI-R2` | Add reusable Agent, Flow, log, approval, table, diff, form, progress, and error patterns with bounded event rates | Slow terminals and large streams remain responsive and memory-bounded |
| `TUI-R3` | Qualify CLI/Code integration, theming, internationalization, screen-reader-friendly modes, and compatibility | Application state remains owned by the host and survives renderer replacement |

TUI does not own Agent sessions or Cloud management state.

## A3S GUI

**Mission:** render structured A3S UI frames through native platform backends
without a browser object model at the host boundary.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `GUI-R1` | Stabilize RSX lowering, portable UI IR, reducers, effects/resources, semantic components, styles, accessibility, and headless tests | AppKit, GTK4, WinUI, and headless hosts agree on event and reducer semantics |
| `GUI-R2` | Complete windowing, text, input, focus, clipboard, drag/drop, routing, collections, overlays, images, and GPU/CPU rendering fallbacks | Visual, accessibility, scaling, IME, crash, and resource-cleanup suites pass per platform |
| `GUI-R3` | Publish host bridges for Code and A3S clients with versioned UI-frame compatibility | Remote or local state remains host-owned; malformed frames cannot execute arbitrary native actions |

GUI is a renderer, not a Cloud Dashboard or domain authority.

## A3S WebView

**Mission:** provide bounded native WebView windows for Code-hosted RemoteUI,
local reports, and Agent Island experiences.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `WEBVIEW-R1` | Stabilize window modes, navigation policy, content/source binding, host messaging, lifecycle, focus, and platform capability discovery | WKWebView, WebKitGTK, and WebView2 behavior is explicit; close/crash removes sessions and temporary resources |
| `WEBVIEW-R2` | Harden origin isolation, content security, download/upload, clipboard, permission prompts, deep links, and untrusted-content boundaries | Hostile content cannot reach host capabilities without an explicit typed grant |
| `WEBVIEW-R3` | Qualify signed UI packages and Code/Use generation handoff | UI generation N retains N bindings until close; N+1 cannot mutate an existing window implicitly |

WebView does not serve public Cloud traffic and does not authorize host actions.

## ash

**Mission:** provide an Agent-first typed shell that schedules bounded process
and RPC graphs and returns compact, referenceable evidence suitable for model
context.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `ASH-R1` | Stabilize the ASH program/ASON result contracts, type checking, process/RPC execution, bounded I/O and CPU planes, sessions, references, cancellation, and cleanup | Grammar, tokenizer-cost, concurrency, large-output, process-tree, and cross-platform suites pass |
| `ASH-R2` | Complete capability-gated workspace materialization, no-overwrite transactions, reference algebra, secret redaction, policy hooks, and audit evidence | Retained results cannot escape authority or overwrite files outside an admitted transaction |
| `ASH-R3` | Publish signed installers, A3S Use/Code integration, protocol compatibility, and six-target release evidence | A clean host verifies, runs, upgrades, recovers, and removes a signed release |

ash does not become A3S Runtime, Box, Flow, or a human-shell compatibility
layer. The host owns permissions and product lifecycle.

## A3S Test

**Mission:** use one typed action/evidence engine for Agent-guided exploration
and deterministic ACL regression suites across Web, GUI, and future TUI
surfaces.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `TEST-R1` | Stabilize observation/action/evidence/session/cleanup contracts and deterministic ACL suite execution | Exploration and regression invoke the same engine; invalid plans and stale observations fail visibly |
| `TEST-R2` | Qualify Browser Web actions, locked CUA GUI profiles, vision/accessibility grounding, MCP sessions, artifacts, and owned-application cleanup | macOS, Windows, and Linux promotion is separate and requires real-host evidence |
| `TEST-R3` | Add TUI driver, distributed Cloud/API workflows, multi-user concurrency, accessibility, network fault, and security testing | Tests correlate all actions with exact app, tenant, release, and evidence identities |
| `TEST-R4` | Publish portable Skill/CLI installers, CI sharding, flake classification, replay bundles, and retention policy | A failed test can be reproduced from bounded evidence without hidden agent state |

Test does not hide a second planner in deterministic suites or treat a
screenshot alone as proof of backend correctness.

## A3S Bench

**Mission:** capture immutable Task and Candidate locks, run Candidates in an
isolated Runtime, invoke the Task-owned Judge, and retain identity-bound
evaluation evidence.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `BENCH-R1` | Stabilize Task/Candidate/Judge locks, runtime-provider contract, submission projection, metric validation, and local result identity | Candidate mutation, judge mismatch, environment drift, timeout, and resource cleanup are detected |
| `BENCH-R2` | Add distributed batches, resumable evaluation, dataset/artifact manifests, cost/resource evidence, statistical summaries, and comparison contracts | Partial worker loss resumes exact cases without duplicate official results |
| `BENCH-R3` | Qualify Cloud-hosted private evaluations, signed official result promotion, anti-tamper evidence, and reproducible public tasks | Official status requires an authorized promoter and exact environment/runtime/artifact digests |

Bench does not implement an Agent Runtime, own a universal Judge, or become a
leaderboard by default.

## Homebrew Tap

**Mission:** publish reviewable Homebrew formulae for exact, signed A3S release
artifacts.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `BREW-R1` | Generate formula updates from signed release manifests with version, platform, URL, SHA-256, dependencies, install, and test blocks | Formula verification rejects mutable URLs, missing checksums, and artifact/manifest drift |
| `BREW-R2` | Add staged promotion, rollback guidance, deprecation, bottle policy, and automated clean-host upgrade tests | Supported macOS/Linux targets install, test, upgrade, and uninstall without residue |

The tap distributes releases; it does not build an independent product or
define compatibility.

## A3S root distribution

**Mission:** pin one compatible A3S component graph and provide bootstrap,
upgrade, repair, rollback, and release metadata for that graph.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `DIST-R1` | Publish a machine-readable compatibility lock covering repositories, protocols, schemas, migrations, artifacts, and minimum platform requirements | Every shipped client and service can report and verify its lock identity |
| `DIST-R2` | Compose installers and component lifecycle without copying component implementation | Clean-host install, partial failure, repair, upgrade, rollback, and uninstall pass on every supported platform |
| `DIST-R3` | Add signed release manifests, SBOM/provenance, vulnerability policy, release channels, mirror policy, and end-of-life rules | Artifacts are reproducible or provenance-bound; compromised or revoked releases fail closed |

The root distribution is a compatibility and delivery owner, not another
Runtime, package Registry, orchestrator, or source-of-truth database.

## Integration exit

This group is ready when a production incident can be traced from Gateway
request through Cloud operation, Flow/Agent/Function/inference execution,
Runtime/Box unit, and kernel evidence; policy generation and enforcement are
explicit; clients expose the same public contract; and one exact signed bundle
can be installed, exercised, upgraded, rolled back within declared limits, and
cleanly removed.

