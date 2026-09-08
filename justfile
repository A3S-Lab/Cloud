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

# Stop the local PostgreSQL, NATS, and registry dependencies
# (prefer `just down` when the API was started with `just up`)
cloud-down:
    ./tools/dev/cloud_down.sh

# Run the typed Cloud CLI without persisting credentials or context
cloud-cli *args:
    bun run --cwd cli src/main.ts {{args}}
