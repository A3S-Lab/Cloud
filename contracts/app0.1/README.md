# APP0.1 Application release contract

This directory contains the checked-in conformance fixture for the implemented
`APP0.1` immutable release authority.

`cloud.application.release.v1` freezes one Applications-owned immutable
publication contract:

- one of exactly six product experiences, with classic Agent and New Agent as
  distinct identities;
- one interaction mode and a closed set of response modes;
- one explicit audience policy;
- one exact Workflow definition/revision plus contract, payload-set, semantic-
  contract-set, input-schema, and output-schema digests; and
- one presentation digest that cannot change Workflow execution semantics.

The contract retains references and digests only. It does not copy a Workflow
graph or payload, create a Flow history, dispatch a provider, store a session,
issue a credential, publish a Gateway route, or create a mode-specific runtime.

The Rust domain implementation parses and generates this fixture only through
`a3s-acl`, rejects unknown or noncanonical fields, binds exact Workflow
admission evidence, preserves immutable release lineage, and prevents an
Application identity from changing between classic Agent, New Agent, or any
other experience. Migration `124` and the Applications PostgreSQL repository
persist the aggregate head and canonical releases through A3S ORM. They verify
the exact Workflow revision's content and payload-set digests, reject mutable
or forked release history, and commit the release, head advance, idempotency
receipt, audit record, and Outbox event in one transaction.

Project authorization before replay, CQRS, REST/OpenAPI `1.42.0`, the maintained
client, CLI, and six Management MCP tools now reuse this exact contract and the
single Applications repository. Component-only `APP0.2-C1/C2` builds and
persists its session contract on these exact release identities through
migration `125`. `APP0.1` completion is not a public delivery or availability
claim: typed WorkflowRun composition, managed delivery, monitoring, enterprise
evidence, and completion of `APP0.2` through `APP0.6` remain open.
