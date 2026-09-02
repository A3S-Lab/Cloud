# A3S Box Runtime Conformance

Cloud consumes the shared A3S Box Runtime driver directly. The provider
contract and its real-host conformance suite therefore have one implementation
and one source of truth in A3S Box rather than a duplicate Cloud provider.

The `box-revision` and `oci-runtime-revision` files pin the exact provider and
shared-kernel runtime pair used by the Cloud gate. The Box revision must match
the `a3s-box-runtime` Git revision in the workspace manifest.

The GitHub workflow builds the pinned Box shim and guest init, installs the
pinned A3S OCI Runtime into a dedicated temporary Box home, and runs every
Runtime profile advertised by the Box driver. The gate runs on a host where no
Docker-compatible daemon is required or contacted. It fails if the provider
leaves a managed unit, shim, runtime owner, mount, socket, or Runtime state
record behind.

The Cloud consumer phase reuses the same exact provider and the existing Flow,
Fleet command lease, Node Agent journal, Runtime receipt, and resource Claim
state machines. Its cleanup interruption probe sends `SIGKILL` after Box has
durably removed a Service but before the Agent records command completion. A
reconstructed Agent and Flow must adopt the byte-identical removal receipt,
keep the prepared Claim capacity held until that evidence is acknowledged,
release it exactly once, reach terminal cancellation, and leave empty Box
state. The workflow retains the consumer logs and one machine-checkable
certification marker with the provider evidence.

### A0.4 real Agent release gate

The A0.4 consumer test creates a published Agent release and deploys it
through the ordinary Workload, Deployment, Operation, Flow, Fleet, and
Runtime path. It builds the exact Code CLI image through the pinned Box
provider, checks the final OCI manifest/config/archive digests, and binds two
Secrets (provider environment plus a mode-0400 signing file). PostgreSQL
records the durable command, acknowledgement, and observation facts. The
probe kills the Box process, reconstructs the control-plane state, verifies
health/readiness/liveness and restart-time Secret rematerialization, then
stops, removes, and cleans every runtime-owned record. A successful run emits
one `A3S_CLOUD_A0_4_REAL_BOX_RELEASE_CERTIFIED` marker containing the pinned
Box/Code revisions and the exact artifact identity. Hosted MCP remains owned
by `MCP0`; this gate does not claim `G0` or hosted MCP availability.

The allocation consumer probe requires Box to advertise CPU, memory, PID, and
execution-timeout controls after the provider phase has passed every profile
derived from those capabilities. When the host qualification advertises
`EphemeralStorage`, the probe also carries an exact byte quota into the
Sandbox writable layer and proves that writes beyond the quota fail closed;
hosts that cannot enforce the bounded layer must not advertise that control.
The probe then carries one inventory-bound Claim through prepare, exact Runtime
binding, reconstructed inspection, pre-fence release rejection, durable stop,
release, removal, and cleanup. The uploaded artifact contains both the
advertised-profile result and the allocation certification marker.

The storage consumer probe binds Box's one Artifact port to the existing node
Artifact manager. It exercises a read-only Artifact mount, a persistent Volume
across driver reconstruction, isolated tmpfs, deterministic Task-output capture
and authenticated publication, exact journal replay, and final removal. The
cleanup gate requires empty Box records, execution directories, VolumeStore
metadata and paths, Secret tmpfs, and node Artifact inventories; retained image
cache and audit evidence are not treated as live workload state.

The build consumer probe uses the same pinned Box revision and the production
Node Agent `BoxBuildStart`/`BoxBuildInspect`/`BoxBuildRemove` adapter. A private
test subprocess completes a real bounded Linux build and uploads its OCI layout
and native cache, then is killed before it can return. A reconstructed executor
must replay the exact output without another logical upload. The probe removes
that operation, clears the sole native cache under the explicitly armed
dedicated home, downloads the immediate-parent cache Artifact, proves native
cache hydration, rebuilds to the exact original OCI manifest descriptor, and
removes again. Revision-bound JSON records every check; build receipts,
operation-owned ImageStore references, and node Artifact files must return to
their pre-test baseline. Shared content-addressed image and layer caches remain
provider-owned reusable state rather than live operations.

The companion Fleet/Flow probe injects loss before persisting nine step
completion events: start dispatch, start acknowledgement, output receipt, and
each cancel/inspect/remove dispatch and acknowledgement. It reconstructs the
Flow engine at every boundary and requires the complete Fleet command object to
remain identical. Validation, publication, attestation, and cleanup counters
must each record one logical effect. Its retained JSON evidence is bound to the
same exact Cloud and Box revisions as the native build probe.

This local build-consumer gate does not replace operator-owned private source,
HTTPS Registry, Vault Transit, OS-process interruption over persistent
Fleet/Flow stores, or published-Workload evidence required to close `G0`.

Real MicroVM and TEE profiles remain hardware-qualified in A3S Box. Cloud does
not reimplement those provider tests.

`install_box_release.sh` installs checksum-pinned Linux x86_64 Box host
libraries and companion artifacts, then builds the Box CLI, A3S OCI CLI, and
A3S OCI Agent from the exact Cloud-pinned revisions. Disposable PostgreSQL,
NATS, Registry, and object-storage fixtures therefore use the same Box and OCI
capability surface as the Cloud provider gate. The script verifies the host
artifact archive and does not require a VM-capable host or another workload
daemon.
