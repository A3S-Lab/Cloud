# 0004: Automations Own New Invocations

Status: Accepted

## Context

Schedules, signed webhooks, and admitted plugin or source events create new
application or Workflow invocations. Flow timers instead advance an already
existing durable run.

## Decision

Automations exclusively owns trigger definitions, immutable revisions, target
pinning, subscriptions, deduplication, concurrency, and misfire policy. Sources
owns provider connection and normalized source facts; A3S Use owns package and
trigger capability lifecycle; Gateway owns ingress policy. Automations calls the
target owner's command with one idempotent exact-release envelope.

Invocation-only trigger descriptors may appear in an authoring graph but are
not Flow steps. Publication asks Automations to publish the corresponding exact
trigger revision.

## Consequences

Workflow, P0, plugins, Sources, and individual applications do not gain a
scheduler or webhook worker. Flow retains only in-run timers and retry/backoff.
