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
provider. The following evidence remains required before `BX0` is verified:

1. Task and Service lifecycle, recovery, logs, resources, stop, remove,
   cancellation, and residue on the exact Box revision.
2. Private networking, typed endpoints, health, Secrets, Artifact/Volume/tmpfs
   mounts, outputs, and registry credentials.
3. The typed Box build boundary with OCI graph, cache, SPDX, SLSA, signing,
   publication, replay, and process-death evidence.
4. A clean-host Cloud, Box, Gateway, and Power loop covering deploy, route,
   observe, update, rollback, inference, stop, removal, and exact cleanup.

No incomplete capability may be represented as supported. Missing host
capability, missing evidence, stale generation, uncertain cleanup, or revision
mismatch fails closed and keeps the owning roadmap gate open.
