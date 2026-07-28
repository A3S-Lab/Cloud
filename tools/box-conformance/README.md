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

Real MicroVM and TEE profiles remain hardware-qualified in A3S Box. Cloud does
not reimplement those provider tests.

`install_box_release.sh` installs checksum-pinned Linux x86_64 runtime artifacts
for Box and A3S OCI Runtime, then builds the Box CLI from the exact Cloud-pinned
revision over those host libraries. Disposable PostgreSQL, NATS, Registry, and
object-storage fixtures therefore use the same Box capability surface as the
Cloud dependency. The script verifies both release archives independently and
does not require a VM-capable host or another workload daemon.
