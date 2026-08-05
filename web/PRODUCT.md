# A3S Cloud Web Console

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

- Platform operators responsible for tenant, project, environment, workload, delivery, edge, and runtime health.
- Agent engineers who publish immutable Agent releases, bind Skills and MCP capabilities, run A3S Code executions, and inspect semantic events.
- Infrastructure and security teams that need self-hosted control, outbound-only nodes, scoped identity, durable audit evidence, and explicit trust boundaries.

## Product Purpose

A3S Cloud is the authenticated operations surface for a self-hosted control plane. It accepts desired-state intent, persists it in PostgreSQL, and exposes durable convergence through A3S Flow, Workloads, Fleet, Runtime, Box, Gateway, and the sole A3S Code Harness.

Success means an operator can identify the current tenant context, understand platform health, perform supported lifecycle actions, and inspect authoritative evidence without using a second orchestration or execution mechanism.

## Positioning

A3S Cloud provides an A3S-native Agent and application platform on operator-owned Linux systems. It replaces the operational responsibilities commonly split between Google AX and Kubernetes without requiring either system or emulating their APIs.

The control plane owns intent, policy, scheduling, rollout, and management state. Outbound-only node agents carry typed commands to the existing A3S Runtime and A3S Box authorities. Agent execution remains owned by `a3s code harness`.

## Operating Context

- Users work inside an organization, project, and environment tenancy hierarchy.
- Primary workspaces are Overview, Workloads, Agents, Delivery, Edge, and Architecture.
- Mutations create durable operations instead of holding an HTTP request open.
- Operators inspect workload convergence, deployment history, logs, BuildRuns, provenance evidence, routes, certificates, Agent conversations, semantic execution events, and active operations.
- The web console shares one typed API client and contract with the CLI and other management surfaces.

## Capabilities and Constraints

- Preserve the existing authentication model based on a scoped organization API token stored only for the browser tab.
- Preserve all current API behavior, deep-link semantics, lifecycle actions, streaming updates, PNG architecture export, and testable accessibility roles.
- Default the product interface to Simplified Chinese, provide a synchronized English switch on sign-in and authenticated surfaces, and persist only the explicit language preference in local storage.
- A3S Code is the sole Agent Harness. The web console may orchestrate and observe it but must not introduce a second Harness, queue, runtime, or execution protocol.
- The web console must distinguish implemented, in-progress, planned, unavailable, empty, loading, and error states honestly.
- Product copy must not invent customers, benchmarks, pricing, availability guarantees, or completed roadmap capabilities.
- The application remains React 19 with Rsbuild and the existing `lucide-react` icon family unless a separately justified dependency change is approved.

## Brand Commitments

- Preserve the product names A3S, A3S Cloud, A3S Code, A3S Flow, A3S Runtime, A3S Box, and A3S Gateway.
- The user explicitly selected the Finogeeks enterprise AI site as the primary visual benchmark: a high-trust white canvas, strong electric-blue brand field, generous spacing, decisive typography, and readable system-architecture storytelling.
- Translate that benchmark into an operations console rather than copying Finogeeks content, identity, logos, or claims.
- Simplified Chinese is the default product language. English is a complete selectable product version; product and protocol names remain unchanged in both languages.

## Evidence on Hand

- Product and feature authority: `../README.md`.
- Technical ownership and lifecycle authority: `../docs/architecture.md`.
- Delivery status and roadmap evidence: `../docs/development-plan.md` and `../ROADMAP.md`.
- Existing web behavior and API integration: `src/` and its tests.
- User-provided visual reference: the Finogeeks homepage screenshot and `https://www.finogeeks.com/`.
- No approved customer logos, testimonials, commercial metrics, or pricing are available and none may be fabricated.

## Product Principles

1. Show authority clearly: every state and action should reveal which A3S component owns it.
2. Make convergence legible: desired state, progress, evidence, and terminal outcomes should be understandable at a glance.
3. Keep one execution path: all Agent runs remain attached to A3S Code and its sole Harness.
4. Preserve operator control: self-hosting, tenancy, scoped identity, and explicit trust boundaries stay visible.
5. Prefer truthful evidence over decorative claims: real state, real identifiers, and real lifecycle data drive the interface.

## Accessibility & Inclusion

The console must remain fully keyboard navigable, expose semantic landmarks and tab relationships, preserve visible focus states, meet WCAG AA contrast, honor reduced-motion preferences, provide explicit mobile layouts for every multi-column surface, and announce the active document language correctly.
