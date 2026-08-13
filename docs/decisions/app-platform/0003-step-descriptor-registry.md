# 0003: Immutable Semantic Step Descriptors

Status: Accepted

## Context

The coarse Workflow step kind identifies a dispatch class. Growing that enum
for every built-in or plugin node would mix product semantics with orchestration
mechanics and make historical plans mutable under registry upgrades.

## Decision

Workflow owns a versioned descriptor registry. Each immutable descriptor
revision declares a stable ID, semantic profile, coarse kind, owner, typed
ports, configuration-schema and default-policy digests, required bindings,
execution class, error/fallback policy, compatibility range, and a separate
presentation digest.

Every compiled `PlanRevision` pins exact descriptor revisions and canonical
configuration digests. An upgrade creates a new Workflow revision. Marketplace
packages register admitted capabilities through A3S Use; they do not become an
independent descriptor or executor authority.

## Consequences

Descriptors validate and compile product meaning in Cloud, while durable runs
continue through Flow. A descriptor cannot call providers directly, store
Secrets, reinterpret a historical plan, or advertise availability without its
owning provider and recovery gates.
