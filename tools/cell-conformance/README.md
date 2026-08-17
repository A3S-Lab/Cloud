# CELL0.3 provider Runtime conformance

This component gate composes the existing A3S Box Runtime provider, Runtime
Service contract, Fleet command journal, and Node Agent operator adapter. It
does not add a Durable Cell scheduler, process manager, provider client,
command store, or lifecycle.

The checked-in release, upstream revision, and multi-platform OCI image digest
pin one celld build. `verify_celld_release.sh` verifies the immutable upstream
tag, the GitHub Actions provenance identity, the OCI index digest, the Linux
x86-64 manifest/config digests, and the image revision/version labels before
Box pulls the image. The product Service profile remains canonical A3S ACL at
`contracts/cell0.3/celld-v0.2.1-service-profile.acl`; the upstream command-line
flags are an implementation detail of the test adapter, not product
configuration.

The existing `Box provider conformance` workflow then runs the real release as
one ordinary Sandbox Service. The test proves:

- the digest-pinned provider reaches healthy through its public readiness
  endpoint and publishes distinct node-local public/internal Runtime endpoints;
- the existing `DurableCellOperatorObserve` command reduces the internal
  `/state` response to six anonymous counters and replays through the sole
  Fleet journal;
- ordinary `RuntimeStop` sends the provider its normal graceful signal;
- ordinary `RuntimeRemove` removes the exact generation; and
- rebuilding the Runtime client does not resurrect the removed Service.

The marker explicitly records `storage=not-certified`. This gate intentionally
runs an empty, in-memory provider process: it does not certify S0 durability,
application deployment, named SQLite state, alarms, WebSockets, Gateway
publication, rolling handoff, or any failure boundary. Those remain the joint
`CELL0.2`, `CELL0.5`, and `CELL0.6` retained gates. A green component run must
not be used to advertise the Durable Cell product as available.

## CELL0.5 bundle publication gate

`run_bundle_publication_gate.sh` is the joint C3 gate. It composes the pinned
publisher ACL, the existing node-bound Execution projection, the existing Box
Task Runtime, the Cloud Artifact and Secret adapters, and the production S0
object-namespace client. The fixture is a deterministic typed Durable Cell
bundle containing a pre-bundled Worker and one SQLite-backed `Counter` class;
it does not install esbuild or create another publication path.

The test requires the same disposable S3-compatible provider variables as the
S0 gate, plus an installed real Box runner. It writes below a unique
`a3s-cloud-tests/cell-bundle-publication/<storage-namespace-id>` prefix. A pass
requires the Task to finish, Fleet redelivery to replay exactly, the S0 adapter
to read and validate both current pointers, the manifest, and exact module
bytes, and the shared test context to delete and re-list the whole prefix as
empty. The retained log is scanned byte-for-byte for every supplied credential
before evidence is accepted.

The manual `Durable Cell bundle publication conformance` workflow is restricted
to `main` and calls the existing Box provider workflow so Box installation,
celld supply-chain verification, image pulling, process cleanup, and evidence
retention stay single-sourced. Its evidence deliberately keeps
`applicationBehaviorCertified`, `gatewayCertified`, and
`faultMatrixCertified` false; those remain C4/C5 work.
