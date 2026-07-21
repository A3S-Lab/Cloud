# Docker Runtime Certification

`run_isolated_docker_gate.sh` certifies the Docker implementation of the A3S
Runtime contract without restarting or reconfiguring the host Docker daemon. It
runs the mandatory Base and Recovery profiles together with every capability
profile advertised by the driver: Networking, Mounts, Health, Resources, Logs,
Security, and Outputs.

## Safety model

The runner creates a unique, labeled namespace for every execution and records
host inventories before and after the gate. Its provider uses:

- a digest-pinned Docker-in-Docker image and registry image;
- a 4 GiB loopback ext4 data disk and a private Unix socket;
- a run-specific tmpfs Secret directory mounted at the same absolute path in
  the provider daemon;
- a run-specific Artifact state directory mounted at the same absolute path in
  the provider daemon, so nested bind sources resolve without exposing any
  broader host path;
- 2 CPUs, 4 GiB of memory, and 2,048 PIDs;
- a dedicated network-keeper container, so provider restart preserves the
  network namespace used for loopback port probes;
- a delegated cgroup v2 subtree inside a private cgroup namespace, so nested
  resource limits work without exposing the host cgroup namespace;
- a bounded TTL and targeted cleanup for containers, networks, mounts, loop
  devices, directories, and Docker network namespaces.

The Recovery profile restarts only the provider container carrying
`a3s.runtime.conformance.provider=true`. The host Docker daemon is never
restarted. The registry uses an explicit bind mount, so its image-declared
volume cannot create an anonymous-volume leak.

## Prepare exact Git worktrees

Create the monorepo-shaped source tree directly on the Linux runner. Do not copy
local build output to the runner.

```bash
root=/var/tmp/a3s-runtime-tests/release-candidate
mkdir -p "$root/apps" "$root/crates"

git clone git@github.com:A3S-Lab/Cloud.git "$root/apps/cloud"
git clone git@github.com:A3S-Lab/Runtime.git "$root/crates/runtime"

git -C "$root/apps/cloud" checkout --detach "$CLOUD_SHA"
runtime_revision=$(<"$root/apps/cloud/tools/runtime-conformance/runtime-revision")
git -C "$root/crates/runtime" checkout --detach "$runtime_revision"
```

Both worktrees must be clean. The Cloud SHA is supplied by the release
candidate, while Cloud pins the compatible full 40-character Runtime SHA in
`tools/runtime-conformance/runtime-revision`.

## Run the gate

The host must already contain the pinned runner images shown by `--help` and a
working Rust toolchain. Run from the Cloud worktree:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA"
```

Omitting `--suite` selects `--suite provider`. This is the release gate for the
Docker `RuntimeDriver`: it runs the mandatory Base and Recovery profiles and
every profile advertised by the driver (Networking, Mounts, Health, Resources,
Logs, Security, and Outputs). The Security profile resolves a file Secret only
inside the driver, proves its value is absent from Runtime specs, provider
inspection, Runtime inspection, and Runtime observation evidence, requires
exact log redaction, then retries and restarts the provider while preserving
one container and one material file. Removal must delete the generation's
material directory.

The Mounts profile keeps a named-volume Service running across a distinct
caller request and isolated provider restart. It requires the same single
container and volume identity after each operation, then mounts that volume
into a separate read-only Task to verify the exact token and write denial. The
Service and named volume must both be removed explicitly. It also materializes
an exact digest-bound directory Artifact, verifies its absolute read-only bind
inside a Task, reconstructs the driver without changing identity, and requires
removal to delete the view and unreferenced blob.

The Outputs profile captures a bounded Task directory as an Artifact, verifies
its URI, digest, media type, size, replay, and reconstructed-driver identity,
rejects an oversized output, detects same-length blob tampering, and requires
removal to delete the output view and unreferenced blob.

The `Docker provider conformance` GitHub Actions workflow runs this provider
gate on relevant pull requests and merges to `main`, every night, and on manual
dispatch. It uses a disposable Ubuntu runner, checks every host prerequisite
before starting, and uploads the complete evidence directory even when the gate
fails. Missing Docker, loop-device, socket, or restart-target prerequisites
fail the job rather than converting certification into a skipped test.

Run the Cloud consumer gate explicitly after the provider gate passes:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA" \
  --suite cloud
```

The Cloud suite adds digest-pinned PostgreSQL and NATS services. It runs
`postgres_foundation_is_migrated_atomic_and_idempotent`, which covers the
persisted projection, command journal, process restart, JetStream redelivery,
reconciliation, cancellation, and cleanup path. It also binds a run-specific
tmpfs Secret directory into the nested provider and exercises real
PostgreSQL-backed Secret authorization/decryption, Docker environment and
`0400` file injection, provider-boundary stdout/stderr redaction, immutable
filesystem log objects, a child control-plane exit after object publication but
before PostgreSQL receipt, reconstructed-adapter orphan adoption and exact
batch replay, deliberate non-secret object corruption, ordered REST gap
readback, and post-test Secret-file cleanup. The provider suite independently
requires a pre-restart Docker log cursor to survive and resume after isolated
daemon replacement. The Cloud suite then runs
`real_docker_updates_preserve_a_failed_candidate_and_rollback_retires_the_current_revision`
against the real isolated Docker provider. It deploys healthy A, proves failed
B cannot replace or stop it, activates a distinct healthy C, and requires the
deterministic Runtime stop for A to reach durable stopped-or-absent evidence.
It then clones A's resolved template into a new generation, activates that
rollback, and requires the deterministic stop for C before the rollback becomes
terminal.

The Cloud suite's network namespace intentionally has no public-network route.
For its PostgreSQL fixture only, the runner injects a deterministic source
resolver that accepts typed full commit references and rejects branch and tag
references. The dedicated GitHub source-resolution and Linux Secret/log CI jobs
retain the production GitHub adapter and provide its live-provider evidence.

The dedicated `Linux Secret and logs` CI job runs the same PostgreSQL test with
an additional digest-pinned registry fixture that requires HTTP basic
authentication. It proves anonymous access fails, removes the local workload
image after publishing it, requires the production control-plane resolver to
decrypt the exact bound credential for manifest authentication, and requires
the node to resolve the same encrypted reference before Docker can pull the
exact private digest.

The dedicated `Runtime BuildKit private Registry` CI job provisions the exact
named volume expected by the Docker Runtime driver, starts the digest-pinned
rootless BuildKit daemon on its shared Unix socket, and rejects anonymous access
to a digest-pinned private Distribution fixture. It then drives the projected
build Task through the real command journal, Docker Runtime, Artifact transport,
OCI validator, and production publisher. Success requires both network-denial
layers, an offline scratch build context containing a bounded root filesystem
exported from the digest-pinned linux/amd64 BusyBox fixture, authenticated
digest-only push, complete remote graph verification, idempotent replay,
Runtime removal, and deletion of the socket volume. The exported root filesystem
keeps BusyBox and its dynamic-loader closure bound to the same image digest. The
manual operator command and fixture contract are documented in the repository
README under `Certify the isolated Build Flow`.

Without `--registry-data`, the runner copies the pinned multi-platform BusyBox
OCI index from Docker Hub into a temporary registry and retries transient copy
failures three times. For an offline or rate-limited runner, pass a persistent
registry data directory that already contains the same pinned root digest:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA" \
  --registry-data /var/tmp/a3s-runtime-fixtures/registry-busybox
```

Every linked manifest and blob in an existing fixture is hashed before use. The
expected fixture contains 18 manifests and 34 blobs under root digest
`sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662`.

## Evidence

The default provider evidence directory is
`/tmp/a3s-cloud-docker-full-isolated-<run-id>`. A successful provider execution
contains `result.txt` with `A3S_DOCKER_CERTIFICATION_PASS` and
`exit-status.txt` with `0`.

The Cloud suite writes
`/tmp/a3s-cloud-runtime-e2e-isolated-<run-id>`. Its success marker is
`A3S_CLOUD_RUNTIME_E2E_PASS`. Its three status files (the aggregate status and
the two individual Cargo test statuses) must all contain `0`, and
`postgres-test-database-count.txt` must contain `0` after the isolated test
database is removed.

Both suites retain:

- exact source and image digests;
- OCI fixture hashes and manifest headers (18 manifests, 34 blobs, and 52
  audited objects for the pinned fixture);
- provider build, pull, test, inspect, and daemon logs;
- provider inventories before and after conformance;
- host container, volume, network, loop-device, mount, and Docker-netns deltas;
- provider/keeper network namespace identities across every restart;
- the run-specific Artifact state root plus post-test and pre-cleanup Artifact
  file findings;
- the run-specific tmpfs Secret path and any post-test Secret-file findings;
  and
- cleanup logs plus targeted post-cleanup leak inventories.

Certification succeeds only when the exact-SHA source worktrees remain clean,
the provider inventory returns to its baseline, every host inventory delta is
empty, the TTL process exits, and the provider root, loop device, mount, and
Docker network namespace leave no residue. The Artifact state root must contain
no files before targeted removal. The Cloud suite additionally
requires its run-specific tmpfs Secret directory to contain no files before
targeted removal.

Default Docker network IDs may rotate across a provider restart. The runner
records that rotation, but leak certification compares stable network semantics
(name, driver, scope, internal flag, and IPv6 flag), not ephemeral IDs.
