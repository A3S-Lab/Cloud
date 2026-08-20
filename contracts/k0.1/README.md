# K0.1 Files and Knowledge contracts

This directory contains checked-in A3S ACL conformance fixtures for the `K0.1`
authority foundation.

`user-file.acl` is the implemented `K0.1-C1` Files component contract. The
`cloud.user-file.v1` schema binds:

- exact organization, project, UserFile, and upload-session identities;
- one derived logical immutable-object reference plus canonical SHA-256 digest,
  bounded size, and media type;
- the original safe basename;
- a bounded upload-session deadline distinct from byte retention; and
- mandatory scan admission before another context can consume the reference.

The Files aggregate owns upload, scan, rejection, expiry, tombstone, and
optimistic-version state. Its typed streaming adapter reuses the process-wide
immutable-object client's verified multipart upload under the `user-files`
child namespace; it does not buffer a whole user file or add another uploader.
The ACL and events contain no provider, bucket, credential, local path, scanner
payload, or file bytes.

`K0.1-C1` is component-only. It does not claim complete `K0.1` or public file
availability. Atomic quota reservation, PostgreSQL persistence, authorization,
idempotency, audit, Outbox, cleanup, REST/OpenAPI, maintained client, CLI, MCP,
and the Knowledge/KnowledgePipeline aggregates remain in the later `K0.1`
sub-gates.
