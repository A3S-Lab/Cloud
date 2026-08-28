# 0078: Keep Git, OCI, A3S Use Registry, and model supply as separate authorities

Status: Accepted

## Context

A3S Cloud requires hosted source code, digest-pinned executable artifacts, and
signed cognitive packages. It also requires governed logical models and very
large immutable weight snapshots. These surfaces are casually called a registry,
but they have different identity, mutability, trust, transport, retention, and
consumer semantics. Combining them behind a universal Cloud Registry would
duplicate existing Git, OCI, and A3S Use mechanisms and make provenance or
revocation ambiguous.

## Decision

The platform keeps three explicit authorities:

| Authority | Content and identity | Ownership boundary |
| --- | --- | --- |
| Hosted Git | Mutable refs plus Git commits, trees, blobs, tags, and Smart HTTP behavior | Cloud Assets owns repositories, ref policy, writer lease, audit, and replicated POSIX topology identity |
| OCI Registry | Immutable OCI blobs/manifests selected by digest | An external standards-compliant registry stores bytes; Cloud Artifacts owns accepted publication, provenance, and exact digest references; Secrets owns credentials |
| A3S Use Registry | Signed TUF roots/metadata, reviewed catalog records, and immutable cognitive-package targets | `a3s-use` owns formats, verification, planning, and package lifecycle; the separate Use Registry repository owns signed publication; Cloud Plugins owns tenant registry enrollment and exact package assignments only |
| Model and weight supply | Logical Model/Revision/WeightVariant metadata plus immutable sharded weights, tokenizer/config, card, license, and provenance | Inference owns model semantics; Artifacts owns the canonical model manifest; S3 owns bytes; Fleet owns cache observations; Power consumes exact admitted revisions |

S3-compatible object storage is not a universal registry. It is the shared
immutable-byte authority used through typed namespaces for artifacts, files,
checkpoints, Web bundles, evidence, and backups. PostgreSQL stores desired
state and content references, never copies of registry indexes or blobs.

A3S Gateway may expose explicitly admitted public Git Smart HTTP, OCI, and Use
Registry routes, but does not implement any registry protocol or mutate their
content. Internal builds and Box pulls prefer private endpoints and
least-privilege credentials.

The official A3S Use Registry may serve immutable metadata/targets through the
`WEB0` object path only after TUF-owned content type, range, digest, expiry, and
mirror conformance passes. Offline root/signing authority remains outside
Cloud and Gateway.

## Consequences

- A source ref can move without weakening digest-pinned runtime publication.
- OCI garbage collection cannot delete Use catalog history or Git objects by
  implication.
- A3S Use trust-root rotation, expiry, rollback protection, and dependency
  planning remain package semantics rather than OCI tag behavior.
- Cloud needs three bounded anti-corruption ports and one shared Secret/object
  substrate, not a universal Registry aggregate, cache, manager, or installer.
- Production installation and disaster-recovery gates must test each service's
  own outage, backup, restore, expiry, and rollback behavior.
