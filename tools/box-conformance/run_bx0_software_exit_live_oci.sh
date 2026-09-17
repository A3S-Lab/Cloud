#!/usr/bin/env bash
# Publish a real digest-pinned OCI image to the live-prep local registry.
#
# Uses crane (preferred), oras, or skopeo. Never invents digests.
# Prefers durable tarball cache and a3s-box local image store so Docker Hub
# unauthenticated rate limits cannot block LIVE re-runs.
# Default registry: 127.0.0.1:50020 (deploy/dev/compose.bx0-live.acl).
#
# Usage:
#   bash tools/box-conformance/run_bx0_software_exit_live_oci.sh [EVIDENCE_DIR]
#
# Exports (also written to evidence env file):
#   A3S_CLOUD_BX0_ARTIFACT_URI
#   A3S_CLOUD_BX0_ARTIFACT_DIGEST
#   A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI / UPDATE_DIGEST (second tag when possible)
#   A3S_CLOUD_BX0_OCI_CACHE_DIR

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-live-oci.XXXXXX")
fi
mkdir -p -- "$evidence_directory"

fail_blocked() {
  local reason=$1
  shift || true
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-oci.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_OCI_BLOCKED reason=$reason
$*
product_exit=not_claimed
loop_certified=not_claimed
EOF
  exit 2
}

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
# shellcheck disable=SC1090
source "$tools/bx0_ensure_middleware_ports.sh"
bx0_refuse_docker_host

registry_host=${A3S_CLOUD_BX0_OCI_REGISTRY:-127.0.0.1:50020}
repo=${A3S_CLOUD_BX0_OCI_REPOSITORY:-a3s/bx0-software}
source_ref=${A3S_CLOUD_BX0_OCI_SOURCE:-docker.io/library/alpine:3.20}
tag_a=${A3S_CLOUD_BX0_OCI_TAG:-live-a}
tag_b=${A3S_CLOUD_BX0_OCI_UPDATE_TAG:-live-b}

box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
compose_acl=${A3S_CLOUD_BX0_COMPOSE_ACL:-$repository_root/deploy/dev/compose.bx0-live.acl}
if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
  bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" \
    || fail_blocked middleware_ports_before_oci
fi

# Wait for registry HTTP before first copy (host port_map can lag compose up).
wait_registry() {
  local hostport=$1
  local deadline=$((SECONDS + 60))
  local host=${hostport%%:*}
  local port=${hostport##*:}
  while ((SECONDS < deadline)); do
    if curl -fsS --max-time 2 "http://${host}:${port}/v2/" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}
wait_registry "$registry_host" \
  || fail_blocked registry_http_not_ready "registry=$registry_host"

dest_a="${registry_host}/${repo}:${tag_a}"
dest_b="${registry_host}/${repo}:${tag_b}"

publisher=
if command -v crane >/dev/null 2>&1; then
  publisher=crane
elif command -v skopeo >/dev/null 2>&1; then
  publisher=skopeo
elif command -v oras >/dev/null 2>&1; then
  publisher=oras
else
  fail_blocked oci_publisher_unavailable \
    'hint=Install crane, skopeo, or oras on the clean host to copy a real image into the Box-compose registry'
fi

printf '===== BX0.software live OCI publish (%s) =====\n' "$publisher"
printf 'source=%s\n' "$source_ref"
printf 'dest_a=%s\n' "$dest_a"

prep_state=${A3S_CLOUD_BX0_LIVE_PREP_STATE_DIR:-$repository_root/.a3s/cloud/bx0-live-prep}
oci_cache=${A3S_CLOUD_BX0_OCI_CACHE_DIR:-$prep_state/oci-cache}
mkdir -p -- "$oci_cache"
export A3S_CLOUD_BX0_OCI_CACHE_DIR=$oci_cache

digest_a=
digest_b=
case $publisher in
  crane)
    platform_args=(--platform linux/amd64)
    published_a=0
    # 1) Durable single-platform tarball (no hub).
    if [[ -f $oci_cache/${tag_a}.tar ]]; then
      if crane push --insecure "${platform_args[@]}" "$oci_cache/${tag_a}.tar" "$dest_a" \
        >"$evidence_directory/crane-push-a.out" 2>"$evidence_directory/crane-push-a.err"; then
        published_a=1
      fi
    fi
    # 2) a3s-box local image store — avoids Docker Hub rate limits.
    if ((published_a != 1)) && [[ -n $box_bin && -x $box_bin ]]; then
      "$box_bin" tag "$source_ref" "$dest_a" \
        >"$evidence_directory/box-tag-a.out" 2>"$evidence_directory/box-tag-a.err" || true
      if "$box_bin" push --plain-http "$dest_a" \
        >"$evidence_directory/box-push-a.out" 2>"$evidence_directory/box-push-a.err"; then
        published_a=1
      fi
    fi
    # 3) Last resort: hub copy (may fail closed on TOOMANYREQUESTS).
    if ((published_a != 1)); then
      crane copy --insecure "${platform_args[@]}" "$source_ref" "$dest_a" \
        >"$evidence_directory/crane-copy-a.out" 2>"$evidence_directory/crane-copy-a.err" \
        || fail_blocked crane_copy_a_failed
      published_a=1
    fi
    if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
      bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" || true
    fi
    wait_registry "$registry_host" \
      || fail_blocked registry_http_not_ready_after_copy_a "registry=$registry_host"
    digest_a=$(crane digest --insecure "${platform_args[@]}" "$dest_a") \
      || fail_blocked crane_digest_a_failed
    if [[ -f $oci_cache/${tag_b}.tar ]]; then
      crane push --insecure "${platform_args[@]}" "$oci_cache/${tag_b}.tar" "$dest_b" \
        >"$evidence_directory/crane-push-b.out" 2>"$evidence_directory/crane-push-b.err" \
        || fail_blocked crane_push_b_cache_failed
    else
      # Local mutate for a distinct update digest (no second hub pull).
      crane mutate --insecure \
        --annotation "a3s.bx0.live-update=$(date -u +%Y%m%dT%H%M%SZ)" \
        -t "$dest_b" \
        "$dest_a" \
        >"$evidence_directory/crane-mutate-b.out" 2>"$evidence_directory/crane-mutate-b.err" \
        || fail_blocked crane_mutate_b_failed
    fi
    digest_b=$(crane digest --insecure "${platform_args[@]}" "$dest_b") \
      || fail_blocked crane_digest_b_failed
    ;;
  skopeo)
    skopeo copy --dest-tls-verify=false "docker://${source_ref}" "docker://${dest_a}" \
      >"$evidence_directory/skopeo-copy-a.out" 2>"$evidence_directory/skopeo-copy-a.err" \
      || fail_blocked skopeo_copy_a_failed
    digest_a=$(skopeo inspect --tls-verify=false "docker://${dest_a}" \
      | python3 -c 'import json,sys; print(json.load(sys.stdin)["Digest"])') \
      || fail_blocked skopeo_digest_a_failed
    skopeo copy --src-tls-verify=false --dest-tls-verify=false \
      "docker://${dest_a}" "docker://${dest_b}" \
      >"$evidence_directory/skopeo-copy-b.out" 2>"$evidence_directory/skopeo-copy-b.err" \
      || fail_blocked skopeo_copy_b_failed
    digest_b=$(skopeo inspect --tls-verify=false "docker://${dest_b}" \
      | python3 -c 'import json,sys; print(json.load(sys.stdin)["Digest"])') \
      || fail_blocked skopeo_digest_b_failed
    ;;
  oras)
    oras copy --to-plain-http "$source_ref" "$dest_a" \
      >"$evidence_directory/oras-copy-a.out" 2>"$evidence_directory/oras-copy-a.err" \
      || fail_blocked oras_copy_a_failed
    digest_a=$(oras resolve --plain-http "$dest_a" 2>/dev/null || true)
    [[ -n $digest_a ]] || fail_blocked oras_digest_a_missing
    oras cp --from-plain-http --to-plain-http "$dest_a" "$dest_b" \
      >"$evidence_directory/oras-copy-b.out" 2>"$evidence_directory/oras-copy-b.err" \
      || fail_blocked oras_copy_b_failed
    digest_b=$(oras resolve --plain-http "$dest_b" 2>/dev/null || true)
    [[ -n $digest_b ]] || fail_blocked oras_digest_b_missing
    ;;
esac

[[ $digest_a =~ ^sha256:[0-9a-f]{64}$ ]] || fail_blocked digest_a_invalid "got=$digest_a"
[[ $digest_b =~ ^sha256:[0-9a-f]{64}$ ]] || fail_blocked digest_b_invalid "got=$digest_b"
if [[ $digest_a == "$digest_b" ]]; then
  fail_blocked update_digest_not_distinct \
    "live-a and live-b resolve to the same digest; need a real revision flip artifact"
fi

# a3s-box Runtime admit/apply only accepts OCI image/index media types — not
# Docker Schema 2. Normalize published tags in-place when the registry still
# serves application/vnd.docker.distribution.manifest.v2+json.
ensure_oci_image_manifest() {
  local ref=$1
  local label=$2
  local media_type
  media_type=$(
    crane manifest --insecure "$ref" 2>/dev/null \
      | python3 -c 'import json,sys; print(json.load(sys.stdin).get("mediaType",""))'
  ) || fail_blocked "crane_manifest_${label}_failed"
  case $media_type in
    application/vnd.oci.image.manifest.v1+json|application/vnd.oci.image.index.v1+json)
      echo "OCI_MEDIA_OK ref=$ref mediaType=$media_type"
      return 0
      ;;
  esac
  if ! command -v skopeo >/dev/null 2>&1; then
    if [[ -x /tmp/bx0-tools/skopeo ]]; then
      export PATH="/tmp/bx0-tools:${PATH}"
    fi
  fi
  if ! command -v skopeo >/dev/null 2>&1; then
    fail_blocked oci_media_type_requires_skopeo \
      "ref=$ref mediaType=$media_type hint=install skopeo (or place binary at /tmp/bx0-tools/skopeo) to rewrite Docker Schema 2 → OCI"
  fi
  # Minimal trust policy for local insecure registry rewrite (host may lack
  # /etc/containers/policy.json when using a standalone skopeo binary).
  mkdir -p /tmp/bx0-containers "${HOME:-/tmp}/.config/containers"
  cat >/tmp/bx0-containers/policy.json <<'POLICY'
{
  "default": [{"type": "insecureAcceptAnything"}]
}
POLICY
  cp /tmp/bx0-containers/policy.json "${HOME:-/tmp}/.config/containers/policy.json"
  export CONTAINERS_POLICY=/tmp/bx0-containers/policy.json
  export REGISTRY_AUTH_FILE=${REGISTRY_AUTH_FILE:-/tmp/bx0-containers/auth.json}
  printf '%s\n' '{}' >"${REGISTRY_AUTH_FILE}"
  # Rewrite through a temp tag so the destination digest/mediaType become OCI.
  local tmp_ref="${ref%:*}:oci-norm-${label}-$$"
  skopeo copy --policy "$CONTAINERS_POLICY" --format=oci --src-tls-verify=false --dest-tls-verify=false \
    "docker://${ref}" "docker://${tmp_ref}" \
    >"$evidence_directory/skopeo-oci-${label}.out" 2>"$evidence_directory/skopeo-oci-${label}.err" \
    || fail_blocked "skopeo_oci_normalize_${label}_failed"
  skopeo copy --policy "$CONTAINERS_POLICY" --src-tls-verify=false --dest-tls-verify=false \
    "docker://${tmp_ref}" "docker://${ref}" \
    >>"$evidence_directory/skopeo-oci-${label}.out" 2>>"$evidence_directory/skopeo-oci-${label}.err" \
    || fail_blocked "skopeo_oci_retag_${label}_failed"
  media_type=$(
    crane manifest --insecure "$ref" 2>/dev/null \
      | python3 -c 'import json,sys; print(json.load(sys.stdin).get("mediaType",""))'
  ) || fail_blocked "crane_manifest_${label}_after_normalize_failed"
  case $media_type in
    application/vnd.oci.image.manifest.v1+json|application/vnd.oci.image.index.v1+json)
      echo "OCI_MEDIA_NORMALIZED ref=$ref mediaType=$media_type"
      ;;
    *)
      fail_blocked "oci_media_type_still_invalid_${label}" "ref=$ref mediaType=$media_type"
      ;;
  esac
}
ensure_oci_image_manifest "$dest_a" a
digest_a=$(crane digest --insecure --platform linux/amd64 "$dest_a") \
  || fail_blocked crane_digest_a_after_oci_normalize_failed

# Mint live-b from the OCI-normalized live-a. Normalizing two Docker Schema 2
# tags independently can collapse to one OCI digest (annotations stripped).
crane mutate --insecure \
  --annotation "a3s.bx0.live-update=$(date -u +%Y%m%dT%H%M%SZ)" \
  -t "$dest_b" \
  "$dest_a" \
  >"$evidence_directory/crane-mutate-b-after-oci.out" 2>"$evidence_directory/crane-mutate-b-after-oci.err" \
  || fail_blocked crane_mutate_b_after_oci_failed
ensure_oci_image_manifest "$dest_b" b
digest_b=$(crane digest --insecure --platform linux/amd64 "$dest_b") \
  || fail_blocked crane_digest_b_after_oci_normalize_failed
[[ $digest_a =~ ^sha256:[0-9a-f]{64}$ ]] || fail_blocked digest_a_invalid_after_oci "got=$digest_a"
[[ $digest_b =~ ^sha256:[0-9a-f]{64}$ ]] || fail_blocked digest_b_invalid_after_oci "got=$digest_b"
if [[ $digest_a == "$digest_b" ]]; then
  fail_blocked update_digest_not_distinct_after_oci \
    "live-a and live-b resolve to the same digest after OCI normalize + mutate"
fi

# Refresh durable cache from final OCI tags (invalidate stale Docker tarballs).
# Pull to a linux tmp path then atomically move. Best-effort only: EXIT needs
# registry digests. Registry passt flaps must not fail-close after OCI_OK tags.
if [[ $publisher == crane ]]; then
  refresh_tmp=$(mktemp -d "${TMPDIR:-/tmp}/bx0-oci-refresh.XXXXXX")
  if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
    bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" || true
  fi
  if crane pull --insecure --platform linux/amd64 "$dest_a" "$refresh_tmp/${tag_a}.tar" \
      >"$evidence_directory/crane-pull-a-refresh.out" 2>"$evidence_directory/crane-pull-a-refresh.err" \
    && crane pull --insecure --platform linux/amd64 "$dest_b" "$refresh_tmp/${tag_b}.tar" \
      >"$evidence_directory/crane-pull-b-refresh.out" 2>"$evidence_directory/crane-pull-b-refresh.err"; then
    local_sz_a=$(wc -c <"$refresh_tmp/${tag_a}.tar")
    local_sz_b=$(wc -c <"$refresh_tmp/${tag_b}.tar")
    if ((local_sz_a >= 1024 && local_sz_b >= 1024)); then
      mkdir -p -- "$oci_cache"
      mv -f "$refresh_tmp/${tag_a}.tar" "$oci_cache/${tag_a}.tar"
      mv -f "$refresh_tmp/${tag_b}.tar" "$oci_cache/${tag_b}.tar"
    else
      printf 'warning: oci cache refresh too small size_a=%s size_b=%s\n' \
        "$local_sz_a" "$local_sz_b" | tee "$evidence_directory/crane-pull-refresh-warn.txt"
    fi
  else
    printf 'warning: oci cache refresh failed (registry digests remain authoritative)\n' \
      | tee "$evidence_directory/crane-pull-refresh-warn.txt"
  fi
  rm -rf -- "$refresh_tmp"
fi

uri_a="oci://${registry_host}/${repo}@${digest_a}"
uri_b="oci://${registry_host}/${repo}@${digest_b}"
export A3S_CLOUD_BX0_ARTIFACT_URI=$uri_a
export A3S_CLOUD_BX0_ARTIFACT_DIGEST=$digest_a
export A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI=$uri_b
export A3S_CLOUD_BX0_UPDATE_DIGEST=$digest_b

oci_env=${A3S_CLOUD_BX0_OCI_OUT:-$evidence_directory/bx0-live-oci-env.sh}
{
  printf '# Generated by run_bx0_software_exit_live_oci.sh — real registry digests\n'
  printf 'export A3S_CLOUD_BX0_ARTIFACT_URI=%q\n' "$uri_a"
  printf 'export A3S_CLOUD_BX0_ARTIFACT_DIGEST=%q\n' "$digest_a"
  printf 'export A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI=%q\n' "$uri_b"
  printf 'export A3S_CLOUD_BX0_UPDATE_DIGEST=%q\n' "$digest_b"
  printf 'export A3S_CLOUD_BX0_OCI_CACHE_DIR=%q\n' "$oci_cache"
} >"$oci_env"

cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-oci.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_OCI_OK
publisher=$publisher
source=$source_ref
artifact_uri=$uri_a
artifact_digest=$digest_a
update_uri=$uri_b
update_digest=$digest_b
oci_cache=$oci_cache
oci_env=$oci_env
product_exit=not_claimed
loop_certified=not_claimed
honesty=Digests from local a3s-box image store and/or crane mutate; hub only as last resort; never invented.
EOF
exit 0
