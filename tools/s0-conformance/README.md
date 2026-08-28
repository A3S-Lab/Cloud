# S0 object namespace conformance

This retained real-provider gate exercises the production
`ImmutableObjectClient::s3` through the typed `IObjectNamespace` and Agent
checkpoint-object ports. It proves conditional create, competing-create
rejection, read-after-create, exact-token overwrite, stale-token rejection,
read-after-overwrite, and verified cleanup, then inventories one unreferenced
Agent checkpoint object, observes its grace period, claims its exact cleanup
lease, removes it, and proves cleanup replay is empty and idempotent against
disposable namespaces on an HTTPS S3-compatible provider.

The S0, Agent checkpoint, and existing immutable-log provider tests share one
test fixture and the same production object client constructor. Cloud retains
no second S3 builder, provider client, credential parser, or object lifecycle.
Every probe uses a unique prefix below `a3s-cloud-tests`; it must remove its
exact objects and observe the namespace empty before a certification marker is
emitted.

Configure an operator-owned disposable bucket and short-lived credentials:

| Environment variable | Requirement |
| --- | --- |
| `A3S_CLOUD_TEST_S3_ENDPOINT` | Credential-free HTTPS origin |
| `A3S_CLOUD_TEST_S3_REGION` | Region; defaults to `us-east-1` |
| `A3S_CLOUD_TEST_S3_BUCKET` | Disposable S3-compatible bucket |
| `A3S_CLOUD_TEST_S3_ACCESS_KEY_ID` | Short-lived test access key |
| `A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY` | Short-lived test secret key |
| `A3S_CLOUD_TEST_S3_SESSION_TOKEN` | Optional short-lived session token |
| `A3S_CLOUD_TEST_S3_VIRTUAL_HOSTED_STYLE` | Optional exact `true` or `false`; defaults to `false` |

Run from the Cloud repository root:

```bash
bash tools/s0-conformance/run_s3_namespace_gate.sh /absolute/evidence/directory
```

The script records the exact Cloud revision, retains both test logs and their
machine-checked certification markers, scans all retained output for every
supplied credential, and writes SHA-256 checksums. The manual
`S0 object namespace provider conformance` workflow invokes the same script and
uploads evidence only after the secret scan succeeds.

HTTP endpoints remain useful for the checksum-pinned CI regression but cannot
emit the external S0 certification markers. A passing gate certifies only the
shared object-namespace capability and Agent checkpoint orphan reconciliation.
Dedicated namespace credential isolation, sealed recovery/restore,
retention/deletion execution, and the joint Durable Cell provider fault matrix
still require their own retained evidence before `CELL0.2` or the service can
be promoted.
