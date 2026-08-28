# Model and Weight Supply Architecture

## 1. Decision

A3S Cloud treats a **model** and its **model weights** as different but
inseparable authorities:

- Inference owns the logical model, immutable semantic revision, task,
  architecture, license/trust decision, lineage, compatibility, publication,
  and deployment eligibility.
- Artifacts owns one immutable content manifest for the exact weight variant,
  tokenizer/configuration files, model card/license objects, provenance, file
  digests, and storage references.
- The deployment-wide S3-compatible object service owns the immutable bytes.
- Fleet owns bounded node-cache observations. A cache is applied state and
  never model/catalog truth.
- A3S Power consumes only an exact admitted model revision and verified weight
  manifest through a Workloads/Fleet/Runtime/Box deployment.

This is the native model-supply path behind `I0.2a`. It preserves ModelScope-
and Hugging Face-like discovery, exact-revision snapshot acquisition, large
file transfer, verification, and caching outcomes without importing their
repository APIs, mutable refs, deployment controllers, or local cache as Cloud
desired state.

## 2. Why model and weights are separate

A logical model answers semantic and governance questions:

- What is this model for, who published it, and under what license?
- Which architecture, tokenizer family, context limit, modality, trust policy,
  and serving backends are compatible?
- Is this a base, fine-tuned, merged, distilled, quantized, or otherwise
  derived revision?
- Which revision may an Inference route or Agent profile use?

A weight artifact answers byte and delivery questions:

- Which exact files and shards are required?
- What are their sizes, media roles, formats, and cryptographic digests?
- Where are the immutable bytes stored and how are they replicated, cached,
  verified, retained, and deleted?
- Does a node have the complete exact variant required by a deployment?

Putting both concerns in one generic Registry aggregate would make a mutable
catalog alias equivalent to an immutable multi-terabyte snapshot. Storing
weights in PostgreSQL, normal Git, a model-route table, or Runtime state is
therefore forbidden.

## 3. Domain vocabulary and ownership

| Concept | Meaning | Sole owner |
| --- | --- | --- |
| `InferenceModel` | Stable tenant-scoped logical model identity and governance head | Inference |
| `ModelRevision` | Immutable semantic revision with exact upstream/provenance, architecture, task/modality, limits, license/trust, lineage, and admitted weight-variant references | Inference |
| `ModelWeightVariant` | Immutable semantic selector such as precision, quantization, format, tokenizer/config set, and compatibility digest; binds one exact artifact manifest | Inference |
| `ModelArtifactManifest` | Canonical sorted file/shard descriptors, total size, root digest, provenance, and object locations | Artifacts |
| `ModelSourceRevision` | Exact resolved upstream provider/repository/commit or operator-upload identity used by one resolution attempt | Inference anti-corruption boundary |
| `ModelResolutionAttempt` | Operation/Flow-coordinated fetch, verify, scan, normalize, publish, and seal attempt | Inference + Operations/Flow |
| `ModelObject` | Immutable content-addressed file or shard in a model namespace | Shared object authority |
| `NodeModelCacheObservation` | Age-bounded proof that one node has verified exact manifest/files and available cache bytes | Fleet |
| `InferenceBackendRevision` | Closed Power compiler/capability profile that admits model/variant compatibility | Inference |
| `InferenceDeploymentRevision` | Exact model revision + weight variant + backend + topology + serving policy | Inference |

There is no mutable `latest` deployment input. An alias may help discovery, but
every resolution, deployment, route, Agent profile, and Workflow plan binds the
exact sealed revision and variant digest.

## 4. Source providers

The first source contract admits a closed provider set:

- ModelScope model repository plus exact commit/revision;
- Hugging Face model repository plus exact commit/revision;
- existing tenant-owned `ModelArtifactManifest`;
- operator-approved S3/object import with an exact supplied manifest; and
- optionally an OCI artifact carrying a model manifest, explicitly typed as
  model content rather than an executable image.

A provider adapter returns bounded metadata, exact resolved revision, a sorted
file inventory, sizes, upstream hashes when available, download capabilities,
license/model-card references, and provenance. Provider branches and tags are
resolved once. Provider APIs, cookies, local paths, LFS pointers, presigned
URLs, mirrors, and cache layout never enter `ModelRevision`.

Provider credentials are exact Secret-version references materialized just in
time to the resolution Task. They are absent from ACL, PostgreSQL semantic
state, artifacts, manifests, logs, events, and retained evidence.

## 5. Resolution and publication flow

```text
Register InferenceModel
  -> request ModelResolutionAttempt
  -> authorize tenant/source/license policy
  -> resolve mutable provider ref to exact upstream revision
  -> freeze bounded remote file inventory
  -> stream/resume files in a Runtime Task through Box
  -> verify every upstream and locally computed digest
  -> inspect format, unsafe code/pickle, license and malware policy
  -> publish create-only ModelObjects to shared S3
  -> atomically seal ModelArtifactManifest + provenance
  -> create immutable ModelRevision and weight variants
  -> optionally prewarm eligible Fleet node caches
  -> admit an InferenceDeployment only from exact sealed identities
```

The Cloud API process never buffers model weights. A Worker coordinates the
Operation/Flow and a bounded finite Runtime Task performs network and byte
work through Box. The Task uses the same Execution, Secret, egress, object,
cancellation, timeout, receipt, and cleanup authorities as other finite work.
It does not introduce a model downloader queue or daemon.

Exact retry is safe only at a declared resumable file/range boundary. A
provider response lost after an unverified write may resume or redownload the
same file, but cannot seal it. Manifest publication occurs only after every
required file is complete and verified. Process death adopts the same attempt,
temporary object set, and file receipts rather than creating another revision.

## 6. Canonical model artifact manifest

An admitted manifest contains at least:

- schema/compiler version, tenant scope, artifact ID, and root digest;
- exact source provider, repository, resolved upstream revision, retrieval
  time, and provider metadata digest;
- model revision and weight-variant identities;
- declared architecture, task/modality, precision/quantization, weight format,
  tokenizer/config compatibility digest, and custom-code policy;
- a canonical path-sorted file list with closed role, media type, byte size,
  SHA-256 digest, and object reference;
- sharding/index relationships and the exact set required for completeness;
- model card, license, notices, tokenizer, vocabulary, generation/chat
  templates, and configuration descriptors where applicable;
- total bytes/file count, provenance/signature/scan evidence references,
  encryption/storage policy, and retention class; and
- parent/base/derivation references for fine-tuned, merged, distilled, or
  quantized variants.

Supported weight formats are closed and capability-gated. Safetensors is the
preferred initial format. GGUF or other formats require a matching backend and
real conformance. Pickle-capable formats and provider-supplied executable code
fail closed unless a separately reviewed isolation/trust profile explicitly
admits them. File extensions alone never establish format or safety.

A shard index is verified against the canonical file set. Missing, extra,
duplicate, case-colliding, traversal, symlink, sparse-file, digest, size, or
media-role ambiguity rejects the entire manifest.

## 7. Storage and distribution

Model objects use a typed child namespace in the one deployment S3 authority.
Writes are create-only by digest; replay verifies size and digest. Object keys
do not contain untrusted repository paths, credentials, or mutable aliases.
Server-side encryption, tenant scope, quota, retention, replication, backup,
restore, and deletion evidence follow the shared object/S0 contracts.

Large-file delivery supports bounded parallel multipart/range transfer,
resumption, integrity verification, and explicit time/byte budgets. Regional
mirrors and CDN/object replicas are applied transport caches. They may serve a
byte only after root/file digest admission and never become alternate manifest
or model authorities.

OCI may carry an admitted model artifact for interoperability, but executable
image manifests and model manifests keep distinct media types, provenance,
retention, and consumers. An OCI tag is never a ModelRevision identity. Git or
Git LFS may be an upstream transport, but Hosted Git is not the production
weight-byte authority.

## 8. Node cache and placement

Fleet reports cache observations keyed by node, cache generation, model
artifact/variant digest, verified bytes, completeness, last verification,
last access bucket, available cache capacity, and health. It reports no host
path, provider token, model contents, or tenant prompt data.

The scheduler may prefer a node with a fresh complete cache only after hard
CPU/GPU, memory, topology, isolation, failure-domain, and quota constraints
pass. Cache locality is a score, never a Resource Claim. A stale cache hit
cannot make an otherwise ineligible node eligible.

Prewarming is an ordinary finite Task bound to a node Claim and exact manifest.
Serving begins only after required files verify and Power readiness converges.
A backend that supports lazy loading or remote paging requires its own bounded
integrity, failure, and performance gate; existence of HTTP range support is
not sufficient.

Eviction is derived from a bounded cache policy and never deletes the object
authority or ModelRevision. A node cache entry in use by a committed serving
Claim is pinned. Partial downloads have separate temporary identity and cannot
be observed as complete. Concurrent downloaders converge through one
node-local digest lock/receipt rather than duplicate bytes.

## 9. Security, license, and multi-tenancy

- Model discovery and revision reads are grant-scoped; private upstream and
  object access use exact tenant/project/environment policy.
- License text/digest, acceptance actor/policy, redistribution constraints,
  and usage restrictions are immutable admission evidence. Cloud does not
  infer legal permission from public downloadability.
- Trust policy records publisher/source, signature/provenance, allowed formats,
  remote-code posture, scan outcome, and reviewer decision.
- Custom code is disabled by default and, when admitted, runs only through the
  selected Box isolation/egress profile. It never executes in API, Worker, or
  model-catalog processes.
- Cross-tenant cache deduplication may share encrypted physical bytes only when
  the storage provider proves isolation and accounting; logical references,
  authorization, retention, and deletion remain tenant-scoped.
- Model card/license/config content may be displayed only through bounded,
  sanitized object reads. No repository HTML/script becomes trusted UI.

## 10. Failure and recovery

| Failure | Required behavior |
| --- | --- |
| Upstream ref changes during resolution | Exact resolved revision and frozen inventory fence the attempt; drift fails instead of mixing snapshots |
| Provider timeout/rate limit | Bounded wait/retry through the same Flow attempt; no second downloader queue |
| Partial or corrupt file | Keep or discard only temporary bytes; never publish or cache as complete |
| Worker/Task death | Adopt exact attempt/file receipts and resume safe ranges; preserve one Operation |
| S3 outage | Publication and new serving fail closed; existing verified node caches may continue only under declared retention policy |
| Object digest mismatch | Quarantine the object/reference, remove affected targets, and require verified repair; never redownload silently under the same evidence |
| Node/cache loss | Placement may choose another complete cache or run one prewarm Task; ModelRevision remains unchanged |
| Upstream deletion | Sealed mirrored revisions remain according to license/retention policy; new resolution fails explicitly |
| License/trust revocation | Block new deployments/routes and execute explicit owning-context drain policy; preserve audit/history without pretending bytes never existed |

## 11. Delivery gates

| Gate | Required outcome |
| --- | --- |
| `I0.2a-MS1` | Freeze Model/Revision/WeightVariant/ArtifactManifest/source-attempt contracts, formats, licenses, errors, limits, and owner ports |
| `I0.2a-MS2` | Resolve exact ModelScope and Hugging Face fixture revisions through isolated Runtime Tasks with Secret/egress controls and no API-process bytes |
| `I0.2a-MS3` | Publish sharded safetensors plus tokenizer/config/card/license objects through external HTTPS S3 with resume, corruption, replay, quota, retention, and cleanup evidence |
| `I0.2a-MS4` | Verify node cache download/concurrency/restart/eviction and cache-aware placement without making cache state authoritative |
| `I0.2a-MS5` | Compile one exact revision/variant into Power, prove backend compatibility/readiness, and serve after process/node/cache loss |
| `I0.2a-MS6` | Prove private-model isolation, license/trust/custom-code denial, signature/provenance, upstream drift/deletion, backup/restore, large-model load, and zero-secret/residue behavior |

## 12. Non-goals

- A universal Git/OCI/Use/model Registry aggregate.
- Model weights, cards, or file lists in PostgreSQL business rows.
- Mutable provider branches/tags or local cache paths as deployment identity.
- A model downloader daemon, queue, scheduler, object client, or cache database
  inside Inference.
- Treating executable OCI images and model weights as the same artifact type.
- Running provider remote code during discovery or in control-plane processes.
- Treating cache presence as durability, authorization, compatibility, or a
  committed GPU/CPU allocation.
- Claiming ModelScope or Hugging Face API, repository, deployment, training, or
  UI compatibility.
