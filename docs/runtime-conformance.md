# Runtime Provider Conformance

The Docker Runtime certification is a dedicated provider gate. The integration
test is deliberately `ignored` during ordinary workspace tests so an absent
Docker prerequisite is visible as skipped rather than silently reported as a
passing provider test.

Run the gate only on a dedicated Linux provider runner:

```bash
export A3S_CLOUD_TEST_DOCKER=1
export A3S_CLOUD_TEST_DOCKER_SOCKET=unix:///run/a3s-runtime-provider/docker.sock
export A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER=a3s-runtime-provider
export A3S_CLOUD_TEST_SECRET_MEMORY_DIR=/dev/shm/a3s-cloud/runtime-provider

cargo test -p a3s-cloud-node-agent \
  --test docker_conformance \
  real_docker_passes_all_advertised_runtime_profiles \
  -- --ignored --exact --nocapture --test-threads=1
```

`A3S_CLOUD_TEST_DOCKER_SOCKET` defaults to
`unix:///var/run/docker.sock`. Recovery certification additionally requires a
restartable, isolated Docker provider. The container named by
`A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER` must expose that socket and carry the
label:

```text
a3s.runtime.conformance.provider=true
```

`A3S_CLOUD_TEST_SECRET_MEMORY_DIR` must be a private tmpfs directory mounted
into that container at the same absolute path. The isolated runner creates and
validates this mount automatically.

Never point the restart target at shared infrastructure. A runner that uses
the host Docker daemon must be disposable and own the daemon restart outside
the test process.

The suite always runs Base and Recovery and derives every other profile from
the driver's reported capabilities. Docker currently activates Networking,
Mounts, Health, Resources, Logs, and Security. Each profile performs provider
inspection and workload-visible behavior checks. The fixture uses a unique
namespace, enforces bounded Docker operations, removes only namespace-owned
containers and volumes, and requires the canonical post-cleanup inventory to
equal its baseline.

Because Docker advertises `SecretReferences`, Security certification also uses
a run-specific tmpfs directory shared with the isolated provider. A file Secret
is resolved only inside the driver and echoed by the workload. The gate requires
the Runtime spec, Docker inspection, Runtime inspection, and Runtime observation
evidence to exclude the value, while logs contain only `[REDACTED]`. A caller
retry and provider restart must retain one provider container and one `0400`
material file; removal must delete the generation directory.

When developing on a dedicated Docker host that cannot safely restart its
daemon, the following non-certifying probe exercises only the advertised
optional behavior and still enforces cleanup and inventory equality:

```bash
A3S_CLOUD_TEST_DOCKER=1 cargo test -p a3s-cloud-node-agent \
  --test docker_conformance \
  real_docker_exercises_advertised_optional_profile_behavior \
  -- --ignored --exact --nocapture --test-threads=1
```

Its result never substitutes for the mandatory Base and Recovery gate.
Set `A3S_CLOUD_TEST_RUNTIME_PROFILE` to one of `networking`, `mounts`,
`health`, `resources`, `logs`, or `security` to run one focused optional
profile during development. Omitting it runs all optional profiles.

Docker log queries page forward from the earliest retained provider record.
The initial request stops after `limit` records, while a cursor request scans
from the preceding provider timestamp boundary until it finds the exact
stream/timestamp/ordinal/digest cursor and then returns the next page. A missing
cursor returns `RuntimeError::LogDiscontinuity` with the exact unit, generation,
requested cursor, and `cursor_lost` reason, never an empty successful page. A
durable unit whose managed Docker source disappeared returns the same typed
boundary with `source_disconnected`; transport and provider availability errors
remain retryable.

Docker does not expose an API for requesting two log records with an identical
daemon nanosecond timestamp. The real profile verifies provider ordering,
unique cursors, and resume behavior. The production cursor/sequence helpers
separately have a deterministic unit case with two records at the exact same
timestamp, proving ordinal disambiguation without modifying Docker's log files.
The real rotation profile removes the managed source, verifies the exact
`source_disconnected` boundary, recreates the same generation, and then verifies
that the old cursor yields the exact `cursor_lost` boundary.

## Cloud Secret and log acceptance

The isolated runner's `--suite cloud` path additionally sets
`A3S_CLOUD_TEST_SECRET_MEMORY_DIR` to a run-specific directory beneath
`/dev/shm`, verifies that the directory is tmpfs-backed, and bind-mounts the
same absolute path into the nested Docker provider. The PostgreSQL integration
gate compiles as the ordinary CI user and runs only its test binary as root,
matching the isolated release runner. This makes the tmpfs source root-owned so
the root workload can read its `0400` file while the container remains
unprivileged with every capability dropped. The gate then:

- authorizes and decrypts an active Secret version through the production
  application handler;
- in the dedicated Linux CI form, binds a separate encrypted registry
  credential, proves anonymous access is rejected, resolves the manifest
  through the production credential-aware control-plane resolver, removes the
  cached fixture image, and pulls its digest from the authenticated private
  registry;
- injects it into a real Docker environment variable and `0400` file without
  placing plaintext in the Runtime command;
- emits it on stdout and stderr and requires provider-boundary redaction;
- persists the sanitized batch as immutable filesystem objects plus PostgreSQL
  metadata, reconstructs the handler/repository/store, and verifies exact
  replay;
- reads the sanitized records through the tenant-authorized REST endpoint; and
- scans control-plane rows, Flow history, node state, and durable log objects
  for plaintext and requires the post-test tmpfs directory to contain no
  Secret files.

This gate proves the real success path and durable replay boundary. It does not
claim provider or control-plane process-death injection, object corruption, or
automatic workload restart after Secret rotation; those remain separate E0
acceptance gates.
