# Runtime Conformance

A3S Cloud has one node-local execution provider: A3S Box. Cloud consumes the
shared `BoxRuntimeDriver` through the provider-neutral A3S Runtime contract and
does not maintain another provider implementation or provider test suite.

## Revision ownership

The release candidate binds four exact repositories:

| Component | Revision source | Responsibility |
| --- | --- | --- |
| A3S Runtime | `tools/runtime-conformance/runtime-revision` and `Cargo.toml` | Provider-neutral lifecycle, records, contracts, and conformance profiles |
| A3S Box | `tools/box-conformance/box-revision` and `Cargo.toml` | Images, execution, networking, mounts, logs, health, resources, attestation, builds, and cleanup |
| A3S OCI Runtime | `tools/box-conformance/oci-runtime-revision` | Shared-kernel Sandbox execution used on hosted Linux runners |
| A3S Cloud | Git commit under test | Desired state, node commands, journals, reconciliation, routing, and evidence |

Every revision is a full 40-character commit. The workspace dependency and
revision file must match. A gate must fail before execution when they differ.

## Provider gate

`.github/workflows/box-conformance.yml` checks out the exact Box and A3S OCI
Runtime revisions, builds the Box shim and guest init, and installs them in a
dedicated temporary Box home. It then runs the single real-provider Runtime
conformance suite owned by A3S Box.

The suite derives its required profiles from the capabilities returned by the
driver. It must exercise every advertised profile and reject missing evidence;
a workflow matrix cannot silently omit a newly advertised capability. The
provider inventory before and after the run must match. Managed units, Runtime
state, shims, runtime owners, mounts, sockets, and temporary execution roots
are all part of cleanup evidence.

Hardware-specific MicroVM and TEE qualification remains in A3S Box. Cloud
records the exact Box evidence but does not duplicate its implementation.

## Cloud consumer gates

Cloud tests own only Cloud behavior above Runtime:

- command leasing, expiry, replay, and acknowledgement ordering;
- durable Runtime receipts and generation fencing;
- current Runtime health observations across apply, journal replay, and live
  inspection without a Cloud-owned probe worker or health registry;
- desired-state reconciliation after Agent and control-plane interruption;
- resource-claim preparation, binding, release, and orphan fencing;
- Artifact transfer receipts and output publication;
- ordered log batches and explicit discontinuities;
- Gateway target publication from typed Runtime endpoints; and
- cleanup completion before terminal Cloud state.

These tests use Runtime fakes for deterministic failure boundaries. A release
claim is made only after the same behavior passes the real Box provider gate
and the clean-host Cloud loop.

## Box-hosted integration fixtures

`tools/box-conformance/install_box_release.sh` installs independently
checksum-pinned Box and A3S OCI Runtime releases. Local development and the C0
cross-surface gates use this release pair to host disposable service fixtures.
The current local profile contains PostgreSQL, NATS, and Registry; C0 runs its
PostgreSQL fixture through the same Box boundary. Product and fixture
configuration is A3S ACL. The fixtures use a dedicated `A3S_HOME` and are
removed through Box.

The fixture release is not provider certification evidence. Provider
certification always builds and tests the exact source revision pinned by the
Cloud release candidate.

## Current migration boundary

`BX0.1` is implemented: Cloud pins the Runtime/Box pair, parses only the closed
ACL `box` block, constructs the shared Box driver directly, and has no fallback
provider. `BX0.2` consumer recovery and hard-resource Claims are verified by the
[dedicated Linux gate](https://github.com/A3S-Lab/Cloud/actions/runs/30425852930).
Deployment cancellation is verified by the
[exact Box run](https://github.com/A3S-Lab/Cloud/actions/runs/30429412890).
That gate intentionally admits one headless Service, projects no ports and no
health probe, and requires an authoritative `RuntimeRemove` result before the
exact `ResourceClaimRelease`, terminal `Cancelled` transition, and
empty-provider assertion. It remains the health-neutral lifecycle baseline.
The
[final interruption gate](https://github.com/A3S-Lab/Cloud/actions/runs/30456965598)
sends `SIGKILL` after the authoritative Box removal but before Agent command
completion. Recovery adopts the exact receipt, retains the prepared Claim until
acknowledgement, releases it exactly once, reaches terminal cancellation, and
leaves empty Box and process state. This completes the `BX0.2` evidence set.

The first `BX0.3` slice pins Runtime-owned typed Service endpoints and consumes
them through one stateless Gateway-origin adapter. The second is based on A3S
Box `c0a3ddb927ada2bbd907c97521fa531b04440eb5`, whose provider suite advertises
and certifies HTTP, TCP, and command health over the existing generation-fenced
port and exec boundaries. Cloud's existing A3S ACL Workload compiler emits the
HTTP Runtime policy; all probe kinds produce the same provider-neutral
`RuntimeHealthObservation`, so Cloud adds no kind-specific consumer or probe
engine.

The dedicated Cloud consumer gate applies a real health-enabled Box Service
through the Node Agent, requires a current `Healthy` result, reconstructs the
Runtime client and Agent executor, and proves the command journal replays the
exact observation. A new inspect command must return a fresh healthy sample
with the same provider identity and typed endpoint. The Edge adapter must
compile that endpoint into the same live Gateway HTTP origin. Removal must then
return an authoritative receipt, inspection must return `NotFound`, the
listener must close, and the workflow's final provider/process inventory must
be empty. This is one lifecycle and observation path, not a second health
worker, scheduler, registry, endpoint authority, or state store.

The third `BX0.3` slice advances the pinned A3S Box revision to
`9fb9bf528f6c648bbecf203de991106fc39bccdb` and requires explicit isolation at
the closed Node Agent ACL boundary. `box.isolation` accepts exactly `microvm` or
`sandbox`; missing, `automatic`, and unknown selections fail before Runtime
construction. The shipped product profile selects MicroVM. Hosted real-provider
Cloud consumer tests select Sandbox explicitly. Both map directly to the sole
shared `BoxRuntimeDriver`, with no automatic downgrade or fallback. This slice
proves selection behavior only; full Sandbox, MicroVM, and TEE certification
remains release-blocking.

The following evidence remains required before `BX0` is verified:

1. Secret materialization, Artifact/Volume/tmpfs mounts, Task outputs, registry
   credentials, allocation evidence, and complete Sandbox/MicroVM/TEE
   isolation certification.
2. The typed Box build boundary with OCI graph, cache, SPDX, SLSA, signing,
   publication, replay, and process-death evidence.
3. A clean-host Cloud, Box, Gateway, and Power loop covering deploy, route,
   observe, update, rollback, inference, stop, removal, and exact cleanup.

No incomplete capability may be represented as supported. Missing host
capability, missing evidence, stale generation, uncertain cleanup, or revision
mismatch fails closed and keeps the owning roadmap gate open.
