#!/usr/bin/env sh
set -eu

node_runner=/usr/local/bin/a3s-workflow-node

if [ -z "${A3S_NODE_ARTIFACT_URI:-}" ]; then
  export A3S_NODE_ARTIFACT_URI="file://${node_runner}"
fi

if [ -z "${A3S_NODE_ARTIFACT_DIGEST:-}" ]; then
  export A3S_NODE_ARTIFACT_DIGEST="sha256:$(sha256sum "$node_runner" | cut -d' ' -f1)"
fi

exec "$@"
