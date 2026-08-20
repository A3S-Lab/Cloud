# APP0.1 Application release contract

This directory contains the checked-in conformance fixture for the
component-only `APP0.1-C1/C2` foundation.

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

`APP0.1-C1/C2` is not a public availability claim. Authorization and maintained
CQRS, REST/OpenAPI, client, CLI, and Management MCP surfaces remain in the
following `APP0.1` slices.
