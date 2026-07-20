# Clean-host E0 release gate

`run_clean_host_gate.sh` is the final process-level acceptance gate for the
single-node E0 release. It starts from exact clean Cloud and Runtime Git
worktrees and exercises released processes rather than an in-process test
fixture.

The scenario performs this sequence:

1. build the control-plane and node-agent release binaries with the locked
   dependency graph;
2. start digest-pinned PostgreSQL and OCI registry fixtures;
3. copy a digest-pinned BusyBox platform image into the local registry and
   remove the host-side image before deployment;
4. start A3S Gateway 1.0.12 with a management-only ACL snapshot;
5. start the control plane with an isolated development ACL configuration;
6. bootstrap one organization, project, environment, and one-time enrollment
   token through the public REST API;
7. start one real outbound node agent against the host Docker provider;
8. deploy and health-check release A, prove exactly one live provider resource,
   and confirm the Gateway TLS listener is still absent;
9. verify a domain claim, publish the route, wait for the exact Gateway
   acknowledgement, and reach release A through verified TLS/SNI;
10. read strictly ordered log records, open the live SSE stream, and reconnect
    from its exact cursor without replay;
11. update to release B through real health, Gateway cutover, activation, and
    deterministic retirement;
12. roll back by cloning release A into a new revision, repeat the exact routed
    cutover, and prove the original A logs remain queryable;
13. stop the workload, require the stop Operation to succeed from durable
    Runtime evidence, and prove no provider unit remains running; and
14. terminate all processes, remove only run-owned Docker resources and private
    state, verify host inventory returns to baseline, and scan the evidence for
    generated credentials.

The generated product and Gateway configuration files are ACL-only. Generated
credentials are supplied through environment variables and are never written
to those files or expected evidence.

## Prepare a clean Linux host

The host requires:

- Docker with access to `/var/run/docker.sock`;
- Rust and Cargo capable of building the pinned Cloud and Runtime revisions;
- A3S Gateway 1.0.12;
- Bash, Curl, Git, GNU `grep`, Python 3, `sha256sum`, and GNU `timeout`; and
- outbound access to fetch the pinned container images and Rust dependencies.

Prepare the monorepo-shaped source tree directly from Git:

```bash
root=/var/tmp/a3s-cloud-release/release-candidate
mkdir -p "$root/apps" "$root/crates"

git clone git@github.com:A3S-Lab/Cloud.git "$root/apps/cloud"
git clone git@github.com:A3S-Lab/Runtime.git "$root/crates/runtime"

git -C "$root/apps/cloud" checkout --detach "$CLOUD_SHA"
runtime_revision=$(<"$root/apps/cloud/tools/runtime-conformance/runtime-revision")
git -C "$root/crates/runtime" checkout --detach "$runtime_revision"
```

Both worktrees must be clean. Run the gate as the dedicated user that owns
Docker access:

```bash
"$root/apps/cloud/tools/release-conformance/run_clean_host_gate.sh" \
  --source-root "$root" \
  --cloud-sha "$CLOUD_SHA" \
  --runtime-sha "$runtime_revision" \
  --gateway "$(command -v a3s-gateway)"
```

The four loopback ports can be overridden when a dedicated host reserves the
defaults:

```bash
"$root/apps/cloud/tools/release-conformance/run_clean_host_gate.sh" \
  --source-root "$root" \
  --cloud-sha "$CLOUD_SHA" \
  --gateway "$(command -v a3s-gateway)" \
  --api-port 28080 \
  --node-control-port 28443 \
  --gateway-port 29443 \
  --gateway-management-port 29090
```

The runner rejects occupied or repeated ports before it creates any fixture.
The host Docker daemon is never restarted or reconfigured.

## Evidence contract

The default evidence directory is
`/tmp/a3s-cloud-clean-host-e0-<run-id>`. A successful execution contains:

- `result.txt` with `A3S_CLOUD_CLEAN_HOST_E0_PASS`;
- `exit-status.txt` with `0`;
- exact Cloud and Runtime SHAs, pinned fixture identities, the resolved local
  workload digest, Gateway version, and release-binary hashes;
- public API responses for bootstrap, enrollment, deployments, route
  acknowledgements, logs, Operations, rollback lineage, and workload stop;
- verified TLS bodies for A, B, and the cloned rollback revision;
- the initial live-log SSE event and the cursor-resumed keepalive;
- one distinct live Docker resource identity for A, B, and rollback, followed
  by an empty running inventory after stop;
- control-plane, node-agent, Gateway, PostgreSQL, registry, build, and runner
  logs; and
- before/after host container, volume, and network inventories plus targeted
  post-cleanup inventories.

`result.txt` is withheld if the scenario, cleanup, source-cleanliness check,
host-inventory comparison, or evidence credential scan fails. PostgreSQL and
registry fixture data remain on container tmpfs mounts. Private node and
Gateway keys, generated credentials, and build output remain under the run
root. Both storage classes are removed before success is recorded.

The dedicated GitHub Actions workflow runs this contract on a disposable
Ubuntu host and uploads the evidence directory even when the gate fails. A
separately managed Linux release host can run the same command against the same
release-candidate SHA.
