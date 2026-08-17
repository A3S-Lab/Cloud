#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_bundle_publication_gate.sh EVIDENCE_DIRECTORY}
[[ $evidence_directory == /* ]]

required=(
  A3S_HOME
  A3S_BOX_OCI_AGENT_PATH
  A3S_BOX_OCI_RUNTIME_PATH
  A3S_CLOUD_TEST_BOX_REVISION
  A3S_CLOUD_TEST_S3_ENDPOINT
  A3S_CLOUD_TEST_S3_BUCKET
  A3S_CLOUD_TEST_S3_ACCESS_KEY_ID
  A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY
)
for name in "${required[@]}"; do
  [[ -n ${!name:-} ]]
done
[[ $A3S_CLOUD_TEST_S3_ENDPOINT == https://* ]]
[[ ${A3S_CLOUD_TEST_S3_VIRTUAL_HOSTED_STYLE:-false} == false ]]

image=$(<"$repository_root/tools/cell-conformance/celld-image")
revision=$(<"$repository_root/tools/cell-conformance/celld-revision")
[[ $image =~ @sha256:[0-9a-f]{64}$ ]]
[[ $revision =~ ^[0-9a-f]{40}$ ]]
image_digest=${image##*@}

mkdir -p -- "$evidence_directory"
cd -- "$repository_root"
cloud_revision=$(git rev-parse HEAD)
printf '%s\n' "$cloud_revision" >"$evidence_directory/cell-bundle-publication-cloud-revision.txt"

cargo_bin=$(command -v cargo)
cargo_home=${CARGO_HOME:-$(dirname "$(dirname "$cargo_bin")")}
rustup_home=$(rustup show home)
target_directory=${RUNNER_TEMP:-/tmp}/cloud-box-target
log="$evidence_directory/cell-bundle-publication.log"

set +e
sudo env \
  PATH="$PATH" \
  HOME="$HOME" \
  CARGO_HOME="$cargo_home" \
  CARGO_TARGET_DIR="$target_directory" \
  RUSTUP_HOME="$rustup_home" \
  A3S_HOME="$A3S_HOME" \
  A3S_BOX_OCI_AGENT_PATH="$A3S_BOX_OCI_AGENT_PATH" \
  A3S_BOX_OCI_RUNTIME_PATH="$A3S_BOX_OCI_RUNTIME_PATH" \
  A3S_CLOUD_TEST_CELL_BUNDLE_PUBLICATION=1 \
  A3S_CLOUD_TEST_CELL_PROVIDER_IMAGE="$image" \
  A3S_CLOUD_TEST_S3_ENDPOINT="$A3S_CLOUD_TEST_S3_ENDPOINT" \
  A3S_CLOUD_TEST_S3_REGION="${A3S_CLOUD_TEST_S3_REGION:-us-east-1}" \
  A3S_CLOUD_TEST_S3_BUCKET="$A3S_CLOUD_TEST_S3_BUCKET" \
  A3S_CLOUD_TEST_S3_ACCESS_KEY_ID="$A3S_CLOUD_TEST_S3_ACCESS_KEY_ID" \
  A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY="$A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY" \
  A3S_CLOUD_TEST_S3_SESSION_TOKEN="${A3S_CLOUD_TEST_S3_SESSION_TOKEN:-}" \
  A3S_CLOUD_TEST_S3_VIRTUAL_HOSTED_STYLE=false \
  A3S_DEPS_STUB="${A3S_DEPS_STUB:-1}" \
  RUST_MIN_STACK=33554432 \
  "$cargo_bin" test --locked -p a3s-cloud-control-plane --lib \
    modules::durable_cells::application::bundle_publication::real_conformance::real_celld_bundle_publication_uses_execution_box_secrets_artifacts_and_s0 \
    -- --ignored --exact --nocapture --test-threads=1 \
    2>&1 | tee "$log"
gate_status=${PIPESTATUS[0]}
set -e

EVIDENCE_LOG="$log" python3 - <<'PY'
import os
from pathlib import Path

log = Path(os.environ["EVIDENCE_LOG"]).read_bytes()
for name in (
    "A3S_CLOUD_TEST_S3_ACCESS_KEY_ID",
    "A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY",
    "A3S_CLOUD_TEST_S3_SESSION_TOKEN",
):
    value = os.environ.get(name, "").encode()
    if value and value in log:
        raise SystemExit(f"retained Durable Cell publication evidence contains {name}")
PY

if (( gate_status != 0 )); then
  exit "$gate_status"
fi

grep --fixed-strings \
  'A3S_CLOUD_CELL0_5_BUNDLE_PUBLICATION_CERTIFIED provider=celld ' \
  "$log" >"$evidence_directory/cell-bundle-publication-certification.txt"
test "$(wc -l <"$evidence_directory/cell-bundle-publication-certification.txt")" -eq 1
certification=$(<"$evidence_directory/cell-bundle-publication-certification.txt")

publisher_profile_digest=$(sed -nE \
  's/.* publisher_profile_digest=(sha256:[0-9a-f]{64}) .*/\1/p' \
  <<<"$certification")
s0_profile_digest=$(sed -nE \
  's/.* s0_profile_digest=(sha256:[0-9a-f]{64}) .*/\1/p' \
  <<<"$certification")
bundle_digest=$(sed -nE \
  's/.* bundle_digest=(sha256:[0-9a-f]{64}) .*/\1/p' \
  <<<"$certification")
version=$(sed -nE 's/.* version=([0-9a-f]{16}) .*/\1/p' <<<"$certification")
[[ $publisher_profile_digest =~ ^sha256:[0-9a-f]{64}$ ]]
[[ $s0_profile_digest =~ ^sha256:[0-9a-f]{64}$ ]]
[[ $bundle_digest =~ ^sha256:[0-9a-f]{64}$ ]]
[[ $version =~ ^[0-9a-f]{16}$ ]]
grep --fixed-strings \
  "revision=$revision image_digest=$image_digest" \
  <<<"$certification" >/dev/null
grep --fixed-strings \
  'task=succeeded replay=exact objects=4 cleanup=verified secrets=ephemeral' \
  <<<"$certification" >/dev/null

jq -n \
  --arg cloud "$cloud_revision" \
  --arg box "$A3S_CLOUD_TEST_BOX_REVISION" \
  --arg providerRevision "$revision" \
  --arg image "$image" \
  --arg publisherProfileDigest "$publisher_profile_digest" \
  --arg s0ProfileDigest "$s0_profile_digest" \
  --arg bundleDigest "$bundle_digest" \
  --arg deploymentVersion "$version" \
  --arg certification "$certification" \
  '{
    schema: "a3s.cloud.cell0.5-bundle-publication-evidence.v1",
    cloudRevision: $cloud,
    boxRevision: $box,
    provider: "celld",
    providerRevision: $providerRevision,
    image: $image,
    publisherProfileDigest: $publisherProfileDigest,
    s0ProfileDigest: $s0ProfileDigest,
    bundleDigest: $bundleDigest,
    deploymentVersion: $deploymentVersion,
    certification: $certification,
    checks: {
      typedBundleArtifact: true,
      exactPublisherProfile: true,
      nodeBoundExecutionTask: true,
      outboundBoxSandbox: true,
      ephemeralCloudSecrets: true,
      exactFleetReplay: true,
      s0PointerVisible: true,
      s0ManifestVisible: true,
      s0ModuleDigestVerified: true,
      boxTaskRemoved: true,
      s0NamespaceCleanup: true,
      credentialScanClean: true
    },
    scope: {
      bundlePublicationCertified: true,
      serviceRuntimeCertifiedElsewhere: true,
      applicationBehaviorCertified: false,
      gatewayCertified: false,
      faultMatrixCertified: false
    }
  }' >"$evidence_directory/cell-bundle-publication.json"

jq -e \
  --arg cloud "$cloud_revision" \
  --arg box "$A3S_CLOUD_TEST_BOX_REVISION" \
  --arg revision "$revision" \
  --arg image "$image" \
  '.cloudRevision == $cloud
   and .boxRevision == $box
   and .providerRevision == $revision
   and .image == $image
   and ([.checks[]] | all)
   and .scope.bundlePublicationCertified == true
   and .scope.applicationBehaviorCertified == false
   and .scope.gatewayCertified == false
   and .scope.faultMatrixCertified == false' \
  "$evidence_directory/cell-bundle-publication.json" >/dev/null

grep --fixed-strings \
  'A3S_CLOUD_CELL0_5_SINGLE_NODE_BEHAVIOR_CERTIFIED provider=celld ' \
  "$log" >"$evidence_directory/cell-single-node-behavior-certification.txt"
test "$(wc -l <"$evidence_directory/cell-single-node-behavior-certification.txt")" -eq 1
behavior_certification=$(<"$evidence_directory/cell-single-node-behavior-certification.txt")
service_profile_digest=$(sed -nE \
  's/.* service_profile_digest=(sha256:[0-9a-f]{64}) .*/\1/p' \
  <<<"$behavior_certification")
service_template_digest=$(sed -nE \
  's/.* service_template_digest=(sha256:[0-9a-f]{64}) .*/\1/p' \
  <<<"$behavior_certification")
[[ $service_profile_digest =~ ^sha256:[0-9a-f]{64}$ ]]
[[ $service_template_digest =~ ^sha256:[0-9a-f]{64}$ ]]
grep --fixed-strings \
  "revision=$revision service_profile_digest=$service_profile_digest service_template_digest=$service_template_digest" \
  <<<"$behavior_certification" >/dev/null
grep --fixed-strings \
  'named_sqlite=verified idle_eviction=verified reactivation=verified alarms=verified websockets=verified cleanup=verified process_death=not-certified gateway=not-certified' \
  <<<"$behavior_certification" >/dev/null

jq -n \
  --arg cloud "$cloud_revision" \
  --arg box "$A3S_CLOUD_TEST_BOX_REVISION" \
  --arg providerRevision "$revision" \
  --arg image "$image" \
  --arg serviceProfileDigest "$service_profile_digest" \
  --arg serviceTemplateDigest "$service_template_digest" \
  --arg certification "$behavior_certification" \
  '{
    schema: "a3s.cloud.cell0.5-single-node-behavior-evidence.v1",
    cloudRevision: $cloud,
    boxRevision: $box,
    provider: "celld",
    providerRevision: $providerRevision,
    image: $image,
    serviceProfileDigest: $serviceProfileDigest,
    serviceTemplateDigest: $serviceTemplateDigest,
    certification: $certification,
    checks: {
      exactWorkloadsServiceProjection: true,
      exactS0PublicationNamespace: true,
      namedSQLiteState: true,
      alarmDelivery: true,
      hibernatableWebSocket: true,
      rpo0OutputGateNotOverridden: true,
      idleEviction: true,
      statefulReactivation: true,
      runtimeServiceRemoved: true,
      s0NamespaceCleanup: true,
      credentialScanClean: true
    },
    scope: {
      namedSQLiteStateCertified: true,
      idleEvictionReactivationCertified: true,
      alarmCertified: true,
      hibernatableWebSocketCertified: true,
      providerProcessDeathCertified: false,
      gatewayCertified: false,
      completeApplicationBehaviorCertified: false,
      faultMatrixCertified: false
    }
  }' >"$evidence_directory/cell-single-node-behavior.json"

jq -e \
  --arg cloud "$cloud_revision" \
  --arg box "$A3S_CLOUD_TEST_BOX_REVISION" \
  --arg revision "$revision" \
  --arg image "$image" \
  '.cloudRevision == $cloud
   and .boxRevision == $box
   and .providerRevision == $revision
   and .image == $image
   and ([.checks[]] | all)
   and .scope.namedSQLiteStateCertified == true
   and .scope.idleEvictionReactivationCertified == true
   and .scope.alarmCertified == true
   and .scope.hibernatableWebSocketCertified == true
   and .scope.providerProcessDeathCertified == false
   and .scope.gatewayCertified == false
   and .scope.completeApplicationBehaviorCertified == false
   and .scope.faultMatrixCertified == false' \
  "$evidence_directory/cell-single-node-behavior.json" >/dev/null

(
  cd -- "$evidence_directory"
  sha256sum \
    cell-bundle-publication-cloud-revision.txt \
    cell-bundle-publication.log \
    cell-bundle-publication-certification.txt \
    cell-bundle-publication.json \
    cell-single-node-behavior-certification.txt \
    cell-single-node-behavior.json \
    >cell-bundle-publication-sha256.txt
)
