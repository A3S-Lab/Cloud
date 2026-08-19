# Box-hosted production baseline

This directory is the single-host, ACL-native installation baseline for the
Cloud control plane. It uses A3S Box directly; it does not add Docker,
Kubernetes, another scheduler, another migration mechanism, or a second Cloud
configuration format. Cloud, Flow, and Boot retain their owner-scoped A3S ORM
ledgers instead of sharing or copying manifests.

The checked-in topology contains:

- one digest-pinned PostgreSQL 17 service and one digest-pinned NATS service;
- the terminating `a3s-cloud-migrate` unit;
- separate API, Worker, and Relay units sharing one closed `cloud.acl`;
- one migration credential projection and one serving credential projection;
- PostgreSQL health before migration and successful migration before serving.

The API unit publishes management HTTP on `A3S_CLOUD_API_PORT` (default
`8080`) and the mTLS node-control endpoint on `A3S_CLOUD_NODE_CONTROL_PORT`
(default `8443`). Restrict both at the host/network boundary to their intended
clients.

The ACL declares `server.role = "all"` as the maximum capability envelope.
Each long-lived Box unit selects `api`, `worker`, or `relay` with the
control-plane binary's `--role` restriction. An ACL that declares a dedicated
role cannot be changed or widened by that argument.

## Required inputs

Use Linux A3S Box at exact revision
`d65b8190df52713d2cf0f2375c97babc22f0da71` or a later release that retains its
Compose `secret_environment` contract. The Cloud image must be digest-pinned
and contain:

- `/usr/local/bin/a3s-cloud-control-plane`;
- `/usr/local/bin/a3s-cloud-migrate`; and
- `/bin/sh` plus `wget` for the checked-in health probes.

Edit `cloud.acl` before deployment to name the real HTTPS S3-compatible
endpoint, bucket, Registry, Vault roles, and source policy. The example
`.invalid` hosts deliberately fail closed. Production Cloud admission requires
external HTTPS S3, authenticated HTTPS Registry publication, and Vault-backed
PKI, encryption, and evidence signing.

Box never creates a Secret mount or falls back to disk. Before `compose up`,
mount a private tmpfs at `<A3S_HOME>/runtime-secrets`, owned by the account that
runs Box and mode `0700`. For an `A3S_HOME` of `/var/lib/a3s-box`:

```bash
sudo install -d -m 0700 -o a3s -g a3s /var/lib/a3s-box/runtime-secrets
sudo mount -t tmpfs \
  -o rw,nosuid,nodev,noexec,mode=0700,uid="$(id -u a3s)",gid="$(id -g a3s)" \
  tmpfs /var/lib/a3s-box/runtime-secrets
```

Provision the equivalent mount during boot before invoking Box. Do not place
the Secret values below in `.env`, an ACL, a command argument, or a systemd
`EnvironmentFile`; `secret_environment` reads only the invoking process's real
environment and immediately projects the values into that tmpfs.

## PostgreSQL identities

On a new `postgres-data` volume, the image first creates
`a3s_cloud_bootstrap` as its required bootstrap superuser. `postgres-init.sh`
then creates exactly two runtime login roles:

- `a3s_cloud_migrator` is created without superuser, role-creation, or
  database-creation capability and owns the database and future schema objects;
- `a3s_cloud_serving` is created without superuser, role-creation, database-
  creation, replication, row-security bypass, or schema-creation authority.

The script transfers database ownership to `a3s_cloud_migrator` and changes
`a3s_cloud_bootstrap` to `NOLOGIN` before initialization completes. PostgreSQL
requires its bootstrap identity to retain the superuser attribute, so disabling
login is the enforceable boundary; no Cloud runtime unit receives that
credential. The initialization script does not grant application access or
install default privileges. After Cloud, Flow, and Boot migrations finish, the
terminating `a3s-cloud-migrate` process is the sole access reconciler for the
ACL-named `a3s_cloud_serving` role. It grants current database/schema/table/
sequence/function access, removes default and extra direct privileges, revokes
database connection/temporary-table access from `PUBLIC`, and keeps every
`a3s_orm_migrations` ledger read-only. The packaged database is dedicated to
Cloud; any separately managed operational role must receive its required
database privileges explicitly.

Set independent passwords and URLs. Percent-encode passwords when embedding
them in a PostgreSQL URL:

```bash
export A3S_HOME=/var/lib/a3s-box
export A3S_CLOUD_IMAGE='registry.example.com/a3s/cloud@sha256:<64-hex-digest>'
export A3S_CLOUD_POSTGRES_BOOTSTRAP_PASSWORD='<one-time-bootstrap-password>'
export A3S_CLOUD_POSTGRES_MIGRATION_PASSWORD='<migration-password>'
export A3S_CLOUD_POSTGRES_SERVING_PASSWORD='<serving-password>'
export A3S_CLOUD_POSTGRES_MIGRATION_URL='postgres://a3s_cloud_migrator:<encoded-password>@postgres:5432/a3s_cloud'
export A3S_CLOUD_POSTGRES_URL='postgres://a3s_cloud_serving:<encoded-password>@postgres:5432/a3s_cloud'
```

The PostgreSQL image runs initialization scripts only for a new data volume.
After successful initialization the bootstrap password cannot open a login
session because its role is `NOLOGIN`. Credential rotation on an existing
installation is an explicit database-administrator operation; deleting the
volume is not a rotation procedure. Rerun `a3s-cloud-migrate` after recreating
the serving role so its current-object grants are restored.

## Role-specific credentials

Export the remaining references required by the checked-in production ACL:

```bash
export A3S_CLOUD_NATS_URL='nats://nats:4222'
export A3S_CLOUD_VAULT_ADDR='https://vault.example.com'
export A3S_CLOUD_VAULT_TOKEN='<vault-token>'
export A3S_CLOUD_S3_ACCESS_KEY_ID='<access-key>'
export A3S_CLOUD_S3_SECRET_ACCESS_KEY='<secret-key>'
export A3S_CLOUD_REGISTRY_CREDENTIAL='<registry-credential>'
export A3S_CLOUD_BOOTSTRAP_TOKEN='<at-least-32-safe-bytes>'
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET='<webhook-secret>'
```

The PostgreSQL initialization unit receives the three role passwords only for
a new volume. The migrator receives only
`A3S_CLOUD_POSTGRES_MIGRATION_URL`. API, Worker, and Relay receive only
`A3S_CLOUD_POSTGRES_URL`; the API has no NATS credential, and the Relay has no
Vault, S3, Registry, bootstrap, or source credential.

## Validate and converge

Run from this directory so relative read-only mounts resolve to the checked-in
ACL and initialization script:

```bash
a3s-box compose -f compose.acl config
a3s-box compose -f compose.acl up -d
```

Every Secret-backed service is refreshed on `compose up`, so a successful
upgrade re-resolves credentials, runs the target image's idempotent A3S ORM
migration coordinator for the Cloud, Flow, and Boot owner manifests, reconciles
the ACL-named serving role against their current objects, and only then replaces
serving units. A rollback may select an older
image only while all later applied migrations remain expand-compatible; this
package does not invent down migrations.

## Remaining H0.4 gates

This baseline does not claim high availability. PostgreSQL and NATS are
single-node dependencies, Gateway is independently versioned, and external
S3/Vault/Registry availability is operator-owned. Clean-host installation,
replicated API/Worker/Relay/Gateway placement, PostgreSQL failover,
backup/restore, contract migration, credential rotation, and retained process-
and node-loss evidence remain release gates.
