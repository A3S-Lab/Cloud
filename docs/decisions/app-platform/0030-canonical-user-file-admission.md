# 0030: Admit user files through one canonical lifecycle

Status: Accepted

## Context

Decision 0005 assigns upload sessions, user-file metadata, scan state, quota,
retention, and typed references to Files while assigning immutable bytes to the
shared object client. It does not yet freeze the exact contract or state
transition by which Applications and Knowledge may safely consume a user file.

Treating a successful object write as admission would bypass malware scanning,
scope checks, retention, and audit. Putting provider paths in a public
reference would leak infrastructure identity and couple stored contracts to an
S0 backend. Giving Files its own provider client would duplicate the existing
immutable-object authority.

## Decision

Files owns one canonical `cloud.user-file.v1` A3S ACL. It binds exact
organization, project, UserFile, and upload-session identities; a safe original
basename; distinct upload expiry and byte-retention times; mandatory scan
policy; and one logical content reference with canonical SHA-256 digest,
bounded byte count, and media type.

The logical object reference is derived from those exact identities and digest.
Provider, bucket, credential, endpoint, local-path, multipart, scanner, and byte
fields are forbidden. Canonical parsing and generation use only `a3s-acl` and
reject unknown fields, noncanonical bytes, nil identities, scope drift, and
reference drift.

The Files aggregate transitions monotonically through `awaiting_upload`,
`awaiting_scan`, and either `admitted` or `rejected`; an unused upload may
become `expired`, and any live state may become `tombstoned`. Every transition
is optimistic-version checked and timestamp bounded. Only `admitted` exposes a
typed content reference. Scan evidence is a digest and closed reason code, not
scanner output or provider data.

Files uses one thin typed adapter over the process-wide
`ImmutableObjectClient` and its `user-files` child namespace. Writes and
verification reuse its bounded stream/multipart path rather than buffering whole
files or implementing another uploader. Exact writes replay idempotently;
verification, conflicts, and corruption fail closed. C1 exposes neither an
unverified download stream nor raw object removal: C2 cleanup must
first prove the persisted tombstone, retention policy, and cleanup fence through
the Files authority. The aggregate advances from upload to scan only with the
adapter's exact reference-bound write receipt, including a replayed receipt
after a database crash gap. It accepts a scan transition only through a
transient receipt binding that same content reference, canonical evidence
digest, and closed decision. The lifecycle event contains bounded metadata
only.

This decision establishes the `K0.1-C1` component boundary. Atomic quota
reservation, PostgreSQL/A3S ORM persistence, authorization-before-replay,
idempotency, audit, Outbox, cleanup, and maintained interfaces remain required
before Files or `K0.1` can be called available. Scanner execution remains with
the admitted execution/provider path; Knowledge owns document and chunk
lineage after consuming an admitted reference.

## Consequences

- Applications and Knowledge cannot interpret an uploaded object as admitted
  or create their own file/blob state.
- Changing tenant, upload identity, digest, or object path changes or invalidates
  the contract instead of silently retargeting bytes.
- Files adds no object provider, scanner, queue, scheduler, Knowledge document,
  application session, or runtime working-file mechanism.
- `K0.1-C1` is not a public upload API or full `K0.1` completion claim.
