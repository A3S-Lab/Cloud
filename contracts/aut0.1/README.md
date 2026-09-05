# AUT0.1 Automation authority contracts

This directory freezes the component-only value contracts for the Automations
authority. The contracts create new invocations; they do not implement a
scheduler, webhook listener, event consumer, queue, or target runtime.

`cloud.automation.definition.v1` contains a closed trigger union (schedule,
webhook, plugin event, or source event), one exact ApplicationRelease,
WorkflowRevision, or Task target, and immutable authorization,
deduplication, concurrency, and misfire policy. The deduplication key is a
closed template whose required identity is checked against the trigger kind.

`cloud.automation.revision.v1` adds contiguous digest-linked immutable
lineage. A successor must point to the immediately previous revision and
must change the canonical trigger intent; mutable `latest` selectors are not
representable.

`cloud.automation.invocation.v1` is the idempotent handoff to the target
owner. It binds the exact revision, origin identity, policy-derived
deduplication key, bounded JSON/object input, and authorization snapshot.
Audit and Outbox values retain only redacted identities and digests, so
provider credentials, raw source payloads, and target-owned run history never
enter the Automations contract.

The fixtures are canonical A3S ACL and are parsed by
`a3s-cloud-contracts`. Webhook signing, schedule evaluation, plugin/source
normalization, persistence, and public interfaces remain later AUT0 gates.
