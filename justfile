# A3S Cloud development and delivery commands

default:
    @just --list

# Start the control-plane API and the hot-reloading web console
cloud:
    ./tools/dev/run_cloud.sh dev

# Build the SPA and serve the API and web console through A3S Gateway
cloud-gateway:
    ./tools/dev/run_cloud.sh gateway

# Stop the local PostgreSQL, NATS, and registry dependencies
cloud-down:
    docker compose --env-file deploy/dev/.env.example --file deploy/dev/compose.yaml down

# Run the typed Cloud CLI without persisting credentials or context
cloud-cli *args:
    bun run --cwd cli src/main.ts {{args}}
