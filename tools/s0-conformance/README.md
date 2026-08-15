# S0 object namespace conformance

This retained real-provider gate exercises the production
`ImmutableObjectClient::s3` through the typed `IObjectNamespace` port. It proves
conditional create, competing-create rejection, read-after-create, exact-token
overwrite, stale-token rejection, read-after-overwrite, and verified cleanup
against one disposable namespace on an HTTPS S3-compatible provider.

The S0 test and the existing immutable-log provider test share one test fixture
and the same production object client constructor. Cloud retains no second S3
builder, provider client, credential parser, or object lifecycle. Every run uses
a unique prefix below `a3s-cloud-tests/s0-cas`; the probe uses a unique key and
must delete it and observe it missing before the certification marker is
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

The script records the exact Cloud revision, retains the test log and its
machine-checked certification marker, scans retained output for every supplied
credential, and writes SHA-256 checksums. The manual
`S0 object namespace provider conformance` workflow invokes the same script and
uploads evidence only after the secret scan succeeds.

HTTP endpoints remain useful for ordinary local regression tests but cannot
emit the S0 certification marker. A passing CAS gate certifies only the shared
object-namespace capability. Dedicated namespace credential isolation, sealed
recovery/restore, retention/deletion execution, and the joint Durable Cell
provider fault matrix still require their own retained evidence before
`CELL0.2` or the service can be promoted.
