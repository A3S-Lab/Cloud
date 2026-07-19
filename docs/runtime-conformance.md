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
