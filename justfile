# A3S Cloud development and delivery commands

default:
    @just --list

# Start the control-plane API (foreground)
cloud:
    ./tools/dev/run_cloud.sh

# One-click: start dependencies + control-plane API (detached)
up:
    ./tools/dev/cloud_up.sh

# One-click: stop detached API + local dependencies
down:
    ./tools/dev/cloud_down.sh

# Start the Use Registry local TUF transport (not the OCI registry from `up`).
# Default: http://127.0.0.1:4873/ — pin --trust-root separately in a3s-use.
# From monorepo root the same recipes are `just up::registry` / `just down::registry`.
up-registry:
    just --justfile "{{ justfile_directory() }}/../../justfile" up::registry

# Stop the Use Registry local TUF transport
down-registry:
    just --justfile "{{ justfile_directory() }}/../../justfile" down::registry

# Stop the local PostgreSQL, NATS, and registry dependencies
# (prefer `just down` when the API was started with `just up`)
cloud-down:
    ./tools/dev/cloud_down.sh

# Run the typed Cloud CLI without persisting credentials or context
cloud-cli *args:
    bun run --cwd cli src/main.ts {{args}}
