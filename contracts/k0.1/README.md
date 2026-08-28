# K0.1 Files and Knowledge contracts

This directory contains checked-in A3S ACL conformance fixtures for the `K0.1`
authority foundation.

`user-file.acl` is the implemented `K0.1-C1`/`C2` Files contract. The
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

`K0.1-C2` adds one `IUserFileRepository` consistency boundary. Migration `170`
atomically commits the aggregate, organization quota allocation, shared audit,
Outbox lifecycle event, and idempotency result. Authorization precedes replay;
Project-restricted Principals see only granted UserFiles, while organization
quota remains an organization-wide view. REST/OpenAPI `1.77.0`, the maintained
TypeScript client, CLI, and five Management MCP tools all dispatch the same
commands and queries.

The maintained surface is metadata-only. It does not expose a binary upload,
download, object removal, scanner, or provider-configuration operation. Public
byte transfer, live scan and cleanup execution, retained PostgreSQL
cross-surface evidence, and the Knowledge/KnowledgePipeline aggregates remain
later `K0.1`/`K0` gates. Therefore neither complete `K0.1` nor public Files
availability is claimed.
