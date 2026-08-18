# A3S Cloud development and delivery commands

default:
    @just --list

# Start the control-plane API
cloud:
    ./tools/dev/run_cloud.sh

# Stop the local PostgreSQL, NATS, and registry dependencies
cloud-down:
    a3s-box compose --file deploy/dev/compose.acl down

# Run the typed Cloud CLI without persisting credentials or context
cloud-cli *args:
    bun run --cwd cli src/main.ts {{args}}
