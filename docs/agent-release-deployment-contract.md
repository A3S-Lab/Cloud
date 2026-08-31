# Agent Release Deployment Contract

This document defines how one hosted Agent commit becomes one immutable A3S
Code release and then one ordinary Runtime Service. It is an ownership and
integrity contract, not a second deployment API.

## Sources of truth

- `.a3s/asset.acl` remains the Cloud-owned Asset manifest.
- `.a3s/agent-release.acl` is the Code-owned release template admitted from the
  exact hosted Git commit.
- The final `a3s.code.agent-release.v1` document is generated only after the
  OCI artifact and signed build provenance exist.
- The final document is mounted read-only as `/app/.a3s/asset.acl`; it is not
  embedded in the image because an image cannot contain a manifest that binds
  the digest of that same image.

Cloud pins the exact A3S Code revision that owns parsing, canonicalization,
compatibility, and publication binding. Cloud does not copy or reinterpret the
Code schema.

## Publication flow

```text
pinned hosted Git commit
  -> admit Cloud Asset manifest and Code release template
  -> deterministic source archive and source-content digest
  -> Box build and complete OCI graph revalidation
  -> immutable OCI publication
  -> canonical SPDX and SLSA provenance
  -> verified DSSE signature
  -> bind OCI digest + source provenance + BuildRun provenance
  -> canonical final Code release manifest
  -> deterministic read-only directory archive
  -> Artifacts hosted-build-outcome v2 fact
  -> Assets immutable Agent release
  -> Workloads immutable revision contract
  -> Runtime Service projection
```

The final manifest has exactly two provenance authorities:

- `source` uses the exact staged source-content digest and its derived Cloud
  source URI;
- `builder` uses the exact BuildRun identity and the digest of the canonical
  signed provenance statement.

Artifact digest, media type, manifest identity, canonical ACL bytes, archive
digest, archive size, source URI, builder URI, and both provenance digests are
revalidated at every persistence or bounded-context restore boundary.

## Deterministic manifest artifact

The manifest mount is a deterministic tar archive containing one entry named
`asset.acl`. The entry has mode `0444`, UID and GID zero, and modification time
zero. Its Artifact URI is derived from its SHA-256 digest. Replay writes the
same bytes to the existing immutable node-artifact authority; no Agent-specific
object store or mutable alias exists.

## Runtime projection

Callers select only declared Secrets and bounded resources. The following
values are derived from the final Code manifest and cannot be overridden:

| Runtime value | Code manifest authority |
| --- | --- |
| command and arguments | `entrypoint` |
| service port | `health.port` |
| readiness probe | `health.readiness_path` |
| manifest mount | exact deterministic archive at `/app/.a3s` |
| workspace mount | `storage.workspace` |
| cache mount | `storage.cache` |
| Secret name and destination | `secret` declarations |

Agent workloads require a positive ephemeral-storage limit. Secret bindings
must match every declared name, target kind, environment variable or absolute
file path exactly; file Secrets use mode `0400`. One separate registry
credential may accompany those declared runtime Secrets. External persistent
data is rejected until a versioned mount target exists.

Skill inputs remain independent, digest-bound read-only Artifact mounts on the
same Workload revision. The final Agent manifest does not absorb Skill release
identity or create another scheduler.

## Upgrade and replay

New publications use `a3s.cloud.hosted-build-outcome.v2` in an Outbox envelope
with schema version 2. The consumer also accepts pending historical v1 facts
only when they have the exact legacy shape: no source-content field and no
Agent final manifest. This lets an upgrade drain committed pre-v2 work without
allowing a new v2 Agent publication to omit its final manifest.

Historical Workload revisions may restore without the new runtime contract so
already-running services remain readable and stoppable. Creating a new Agent
deployment requires a final manifest and fails closed for a historical release
that does not have one.

## Failure rules

Publication or admission fails closed when any of these facts change:

- canonical ACL bytes or manifest identity;
- OCI digest or media type;
- source or builder provenance authority;
- archive URI, digest, size, entry path, mode, ownership, timestamp, or bytes;
- caller-selected process, port, or health policy;
- required Secret mapping;
- storage bounds or unsupported persistent-data mode;
- Outbox payload/envelope version or aggregate identity.

The focused regression suite covers deterministic replay, stored archive
bytes, provenance and archive tampering, v1/v2 Outbox replay, PostgreSQL
round-trip, Runtime mount projection, caller override rejection, Secret
mapping, and storage bounds. Real-provider release certification remains a
separate retained gate and must not be inferred from component tests.
