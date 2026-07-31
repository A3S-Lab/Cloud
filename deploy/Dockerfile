FROM rust:1.88-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY cli ./cli
COPY crates ./crates
COPY node-runner ./node-runner
COPY runtime-provider ./runtime-provider
COPY server ./server
RUN cargo build --release --locked --workspace

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 a3s \
    && install -d -o a3s -g a3s /var/lib/a3s/workflow /var/lib/a3s/runtime

COPY --from=builder /src/target/release/a3s-workflow-node /usr/local/bin/a3s-workflow-node
COPY --from=builder /src/target/release/a3s-workflow-server /usr/local/bin/a3s-workflow-server
COPY --from=builder /src/target/release/a3s-workflow-runtime-provider /usr/local/bin/a3s-workflow-runtime-provider
COPY --from=builder /src/target/release/worker /usr/local/bin/a3s-workflow-worker
COPY config /etc/a3s-workflow
COPY deploy/entrypoint.sh /usr/local/bin/a3s-workflow-entrypoint

RUN chmod 0555 /usr/local/bin/a3s-workflow-entrypoint

ENV A3S_WORKFLOW_CONFIG=/etc/a3s-workflow/workflow.acl \
    A3S_RUNTIME_CONFIG=/etc/a3s-workflow/runtime-provider.acl \
    A3S_WORKFLOW_AUDIT_PATH=/var/lib/a3s/workflow/flow-events.jsonl \
    A3S_RUNTIME_STATE_PATH=/var/lib/a3s/runtime/state \
    A3S_RUNTIME_ARTIFACT_PATH=/var/lib/a3s/runtime/artifacts

USER a3s
WORKDIR /var/lib/a3s

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/a3s-workflow-entrypoint"]
CMD ["a3s-workflow-server"]
