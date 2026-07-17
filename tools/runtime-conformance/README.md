# Docker Runtime Certification

`run_isolated_docker_gate.sh` certifies the Docker implementation of the A3S
Runtime contract without restarting or reconfiguring the host Docker daemon. It
runs the mandatory Base and Recovery profiles together with every capability
profile advertised by the driver: Networking, Mounts, Health, Resources, Logs,
and Security.

## Safety model

The runner creates a unique, labeled namespace for every execution and records
host inventories before and after the gate. Its provider uses:

- a digest-pinned Docker-in-Docker image and registry image;
- a 4 GiB loopback ext4 data disk and a private Unix socket;
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
git -C "$root/crates/runtime" checkout --detach "$RUNTIME_SHA"
```

Both worktrees must be clean, and the two full 40-character SHAs are mandatory.

## Run the gate

The host must already contain the pinned runner images shown by `--help` and a
working Rust toolchain. Run from the Cloud worktree:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA" \
  --runtime-sha "$RUNTIME_SHA"
```

Omitting `--suite` selects `--suite provider`. This is the release gate for the
Docker `RuntimeDriver`: it runs the mandatory Base and Recovery profiles and
every profile advertised by the driver (Networking, Mounts, Health, Resources,
Logs, and Security).

Run the Cloud consumer gate explicitly after the provider gate passes:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA" \
  --runtime-sha "$RUNTIME_SHA" \
  --suite cloud
```

The Cloud suite adds digest-pinned PostgreSQL and NATS services. It runs
`postgres_foundation_is_migrated_atomic_and_idempotent`, which covers the
persisted projection, command journal, process restart, JetStream redelivery,
reconciliation, log transport, cancellation, and cleanup path, followed by
`permanently_unhealthy_real_docker_update_preserves_healthy_revision` against
the real isolated Docker provider.

Without `--registry-data`, the runner copies the pinned multi-platform BusyBox
OCI index from Docker Hub into a temporary registry and retries transient copy
failures three times. For an offline or rate-limited runner, pass a persistent
registry data directory that already contains the same pinned root digest:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA" \
  --runtime-sha "$RUNTIME_SHA" \
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
- provider/keeper network namespace identities across every restart; and
- cleanup logs plus targeted post-cleanup leak inventories.

Certification succeeds only when the exact-SHA source worktrees remain clean,
the provider inventory returns to its baseline, every host inventory delta is
empty, the TTL process exits, and the provider root, loop device, mount, and
Docker network namespace leave no residue.

Default Docker network IDs may rotate across a provider restart. The runner
records that rotation, but leak certification compares stable network semantics
(name, driver, scope, internal flag, and IPv6 flag), not ephemeral IDs.
