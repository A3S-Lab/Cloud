#!/usr/bin/env bash
set -euo pipefail

readonly repository_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly pin_directory="$repository_root/tools/cell-conformance"
evidence_directory=${1:?usage: verify_celld_release.sh ABSOLUTE_EVIDENCE_DIRECTORY}

if [[ $evidence_directory != /* ]]; then
  printf '%s\n' 'evidence directory must be absolute' >&2
  exit 2
fi
for command_name in curl gh git jq sha256sum; do
  command -v "$command_name" >/dev/null 2>&1 || {
    printf 'required command is unavailable: %s\n' "$command_name" >&2
    exit 1
  }
done

release=$(<"$pin_directory/celld-release")
revision=$(<"$pin_directory/celld-revision")
image=$(<"$pin_directory/celld-image")
[[ $release =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
[[ $revision =~ ^[0-9a-f]{40}$ ]]
[[ $image =~ ^ghcr\.io/denoland/celld@sha256:[0-9a-f]{64}$ ]]

remote_revision=$(git ls-remote https://github.com/denoland/celld.git \
  "refs/tags/$release" | awk 'NR == 1 { print $1 }')
[[ $remote_revision == "$revision" ]]

mkdir -p -- "$evidence_directory"
printf '%s\n' "$release" >"$evidence_directory/celld-release.txt"
printf '%s\n' "$revision" >"$evidence_directory/celld-revision.txt"
printf '%s\n' "$image" >"$evidence_directory/celld-image.txt"

token=$(curl --fail --silent --show-error --get \
  --data-urlencode 'scope=repository:denoland/celld:pull' \
  --data-urlencode 'service=ghcr.io' \
  https://ghcr.io/token | jq -er '.token | select(length > 0)')
authorization="Authorization: Bearer $token"
image_digest=${image##*@}
index_file="$evidence_directory/celld-index.json"
curl --fail --location --silent --show-error \
  --header "$authorization" \
  --header 'Accept: application/vnd.oci.image.index.v1+json' \
  "https://ghcr.io/v2/denoland/celld/manifests/$image_digest" \
  >"$index_file"
printf '%s  %s\n' "${image_digest#sha256:}" "$index_file" |
  sha256sum --check --strict

# Verify the exact index bytes that were just digest-checked. This keeps the
# provenance query independent of local GHCR credential-helper state while
# retaining the immutable OCI subject digest as the trust anchor.
gh attestation verify "$index_file" \
  --repo denoland/celld \
  --cert-identity "https://github.com/denoland/celld/.github/workflows/release.yml@refs/tags/$release" \
  --deny-self-hosted-runners \
  --format json >"$evidence_directory/celld-attestation.json"
jq -e 'type == "array" and length > 0' \
  "$evidence_directory/celld-attestation.json" >/dev/null

jq -e '
  .schemaVersion == 2 and
  .mediaType == "application/vnd.oci.image.index.v1+json" and
  ([.manifests[] | select(
    .mediaType == "application/vnd.oci.image.manifest.v1+json" and
    .platform.os == "linux" and
    .platform.architecture == "amd64" and
    (.annotations["vnd.docker.reference.type"] // "") == ""
  )] | length == 1)
' "$index_file" >/dev/null

manifest_digest=$(jq -er '
  [.manifests[] | select(
    .mediaType == "application/vnd.oci.image.manifest.v1+json" and
    .platform.os == "linux" and
    .platform.architecture == "amd64" and
    (.annotations["vnd.docker.reference.type"] // "") == ""
  )] | if length == 1 then .[0].digest else error("missing exact amd64 manifest") end
' "$index_file")
manifest_file="$evidence_directory/celld-amd64-manifest.json"
curl --fail --location --silent --show-error \
  --header "$authorization" \
  --header 'Accept: application/vnd.oci.image.manifest.v1+json' \
  "https://ghcr.io/v2/denoland/celld/manifests/$manifest_digest" \
  >"$manifest_file"
printf '%s  %s\n' "${manifest_digest#sha256:}" "$manifest_file" |
  sha256sum --check --strict

config_digest=$(jq -er \
  '.config.digest | select(startswith("sha256:") and length == 71)' \
  "$manifest_file")
config_file="$evidence_directory/celld-amd64-config.json"
curl --fail --location --silent --show-error \
  --header "$authorization" \
  "https://ghcr.io/v2/denoland/celld/blobs/$config_digest" \
  >"$config_file"
printf '%s  %s\n' "${config_digest#sha256:}" "$config_file" |
  sha256sum --check --strict
jq -e --arg revision "$revision" --arg version "${release#v}" '
  .config.Labels["org.opencontainers.image.revision"] == $revision and
  .config.Labels["org.opencontainers.image.version"] == $version and
  .config.Labels["org.opencontainers.image.title"] == "celld"
' "$config_file" >/dev/null

jq -n \
  --arg release "$release" \
  --arg revision "$revision" \
  --arg image "$image" \
  --arg manifestDigest "$manifest_digest" \
  --arg configDigest "$config_digest" \
  '{
    schema: "a3s.cloud.cell0.3-provider-supply-chain-evidence.v1",
    provider: "celld",
    release: $release,
    revision: $revision,
    image: $image,
    linuxAmd64ManifestDigest: $manifestDigest,
    configDigest: $configDigest,
    checks: {
      immutableTag: true,
      githubActionsProvenance: true,
      imageIndexDigest: true,
      linuxAmd64ManifestDigest: true,
      imageConfigDigest: true,
      revisionLabel: true,
      versionLabel: true
    }
  }' >"$evidence_directory/celld-supply-chain.json"
jq -e '([.checks[]] | all)' \
  "$evidence_directory/celld-supply-chain.json" >/dev/null

(
  cd -- "$evidence_directory"
  sha256sum \
    celld-release.txt \
    celld-revision.txt \
    celld-image.txt \
    celld-attestation.json \
    celld-index.json \
    celld-amd64-manifest.json \
    celld-amd64-config.json \
    celld-supply-chain.json \
    >celld-supply-chain-sha256.txt
)
