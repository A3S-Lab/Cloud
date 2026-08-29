# A3S OS Cloud Console

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- Platform operators responsible for tenant, project, environment, workload, delivery, edge, and runtime health.
- Agent engineers who publish immutable Agent releases, bind Skills and MCP capabilities, run heterogeneous Agent providers including the native A3S Code provider, and inspect semantic events.
- Infrastructure and security teams that need self-hosted control, outbound-only nodes, scoped identity, durable audit evidence, and explicit trust boundaries.
- Enterprise AI owners evaluating how unified access, autonomous workflow, Agent production, and security operations fit one governed platform.

## Product Purpose

A3S OS is the public product identity. Its authenticated Cloud console is an operations surface, not a separate product. The outward product layer has three pillars: a unified gateway formed by the Cloud API management plane and A3S Gateway data plane, ontology-driven autonomous workflow orchestration, and a heterogeneous Agent Factory. Sentry, AnySentry, audit, and security response remain first-class capabilities inside the unified gateway product rather than becoming a fourth control plane.

All three products accept governed tenant intent, persist authoritative business state in PostgreSQL, and reuse durable convergence through Operations, A3S Flow, Workloads, Fleet, Runtime, Box, and Gateway. Agent Factory uses one provider-neutral Agent execution contract; A3S Code is its native provider. The products never create independent controllers, schedulers, runtimes, node channels, queues, Agent lifecycles, or evidence stores.

Success means a visitor can understand the three application products, the complete 19-gate portfolio, current delivery boundaries, and the shared runtime architecture before signing in. An authenticated operator can then identify tenant context, understand platform health, perform supported lifecycle actions, and inspect authoritative evidence without using a second orchestration or execution mechanism.

## Positioning

A3S OS provides an A3S-native enterprise AI operating system on operator-owned Linux systems. Unified Gateway gives Workflow, Agent, MCP, model, and business-service traffic one governed cloud-edge entry. Workflow turns business ontology into executable and recoverable processes. Agent Factory turns heterogeneous Agent implementations into versioned, deployable assets. Security operations correlate request, runtime, Agent, and host evidence under the unified gateway's tenant and identity model.

The control plane owns intent, policy, scheduling, rollout, and management state. Outbound-only node agents carry typed commands to the existing A3S Runtime and A3S Box authorities. Agent execution follows one `AgentExecutionProvider` lifecycle; `a3s code harness` remains the native provider command while additional Harnesses must pass the same contract and conformance suite.

## Operating Context

- Users work inside an organization, project, and environment tenancy hierarchy.
- Unauthenticated visitors land on a Chinese-first project homepage that explains the three products, all roadmap gates, the complete module architecture, documentation versions, and the control-plane sign-in path.
- Primary workspaces are Overview, Workloads, Agents, Delivery, Edge, and Architecture.
- Mutations create durable operations instead of holding an HTTP request open.
- Operators inspect workload convergence, deployment history, logs, BuildRuns, provenance evidence, routes, certificates, Agent conversations, semantic execution events, and active operations.
- The web console shares one typed API client and contract with the CLI and other management surfaces.

## Capabilities and Constraints

- Preserve the existing authentication model based on a scoped organization API token stored only for the browser tab.
- Preserve all current API behavior, deep-link semantics, lifecycle actions, streaming updates, PNG architecture export, and testable accessibility roles.
- Default the product interface to Simplified Chinese, provide a synchronized English switch on sign-in and authenticated surfaces, and persist only the explicit language preference in local storage.
- Publish every roadmap gate on the homepage and in documentation through one shared catalog. Preserve the authoritative Verified, In progress, Box re-certification, Planned, and explicit unavailable distinctions instead of presenting a subjective completion percentage.
- Support a persistent documentation-line selector for `main` and `v0.1.x`. `main` follows the current roadmap snapshot; `v0.1.x` describes package `0.1.0` and REST v1 contract `1.6.0` without implying that all roadmap gates are verified.
- A3S Code is the native Agent Harness. The web console may orchestrate and observe any conforming provider through the single `AgentExecutionProvider` lifecycle, but must not introduce a provider-specific controller, queue, scheduler, Runtime, Cloud run store, or execution protocol.
- Treat the website architecture as an additive product projection, not an exhaustive component inventory. It cannot retire tenancy, projects, identity, Sources and builds, ordinary Executions, Assets and A3S Use, Workloads and Fleet, Runtime and Box, Edge and Gateway, Secrets, Operations, Search, audit, stateful storage, high availability, updates, rollback, backup, or disaster recovery merely because a product card does not name them.
- The web console must distinguish implemented, in-progress, planned, unavailable, empty, loading, and error states honestly.
- Product copy must not invent customers, benchmarks, pricing, availability guarantees, or completed roadmap capabilities.
- The application remains React 19 with Rsbuild and the existing `lucide-react` icon family unless a separately justified dependency change is approved.

## Brand Commitments

- Preserve the product names A3S OS, A3S Code, A3S Flow, A3S Runtime, A3S Box, and A3S Gateway. Refer to this authenticated surface as the Cloud console.
- The user explicitly selected the Finogeeks enterprise AI site as the primary visual benchmark: a high-trust white canvas, strong electric-blue brand field, generous spacing, decisive typography, and readable system-architecture storytelling.
- Translate that benchmark into an operations console rather than copying Finogeeks content, identity, logos, or claims.
- Simplified Chinese is the default product language. English is a complete selectable product version; product and protocol names remain unchanged in both languages.

## Evidence on Hand

- Product and feature authority: `../README.md`.
- Technical ownership and lifecycle authority: `../docs/architecture.md`.
- Delivery status and roadmap evidence: `../docs/development-plan.md` and `../ROADMAP.md`.
- Existing web behavior and API integration: `src/` and its tests.
- User-provided visual reference: the Finogeeks homepage screenshot and `https://www.finogeeks.com/`.
- User-provided product hierarchy: Unified Gateway, Workflow, and Agent Factory are the three outward-facing application products; security operations remain inside Unified Gateway, while A3S Runtime and the other A3S components form the shared infrastructure.
- No approved customer logos, testimonials, commercial metrics, or pricing are available and none may be fabricated.

## Product Principles

1. Lead with customer outcomes: trusted unified access, autonomous business orchestration, industrialized Agent production, and governed security operations.
2. Show authority clearly: every product capability, state, and action reveals which A3S component owns it.
3. Make convergence legible: desired state, progress, evidence, and terminal outcomes are understandable at a glance.
4. Keep one execution path: every Agent provider uses the same Cloud lifecycle, while A3S Code remains the native provider rather than the only supported Harness.
5. Preserve operator control: self-hosting, tenancy, scoped identity, and explicit trust boundaries stay visible.
6. Prefer truthful evidence over decorative claims: real roadmap gates, state, identifiers, and lifecycle data drive the interface.

## Accessibility & Inclusion

The console must remain fully keyboard navigable, expose semantic landmarks and tab relationships, preserve visible focus states, meet WCAG AA contrast, honor reduced-motion preferences, provide explicit mobile layouts for every multi-column surface, and announce the active document language correctly.
