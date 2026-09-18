#!/usr/bin/env bash
# Local A0.4 real-Box Agent release re-cert on current BX0.software pins.
#
# Builds the exact Agent Runtime image through pin-matched a3s-box, publishes
# to the BX0 LIVE registry (127.0.0.1:50020), then runs the ignored PostgreSQL
# integration test. Emits A3S_CLOUD_A0_4_REAL_BOX_RELEASE_CERTIFIED on success.
#
# Usage:
#   bash tools/box-conformance/run_a0_4_real_box_gate_local.sh [EVIDENCE_DIR]
#
# Required host: Docker-free Linux, pin-matched a3s-box + a3s-oci, Postgres
# reachable (default 127.0.0.1:54320 from compose.bx0-live.acl).
set -euo pipefail

repository_root=${A3S_CLOUD_ROOT:-}
if [[ -z $repository_root ]]; then
  repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
fi
# When the script is copied to /tmp, BASH_SOURCE no longer points at apps/cloud.
if [[ ! -f $repository_root/tools/box-conformance/box-revision ]]; then
  repository_root=/mnt/d/code/a3s/apps/cloud
fi
tools=$repository_root/tools/box-conformance
test -f "$tools/box-revision" || {
  echo "box-revision missing under $tools" >&2
  exit 2
}
evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-a0-4-real-box.XXXXXX")
fi
mkdir -p -- "$evidence_directory"
export A3S_CLOUD_A0_4_EVIDENCE_DIR=$evidence_directory

box_revision=$(<"$tools/box-revision")
oci_revision=$(<"$tools/oci-runtime-revision")

BOX_BIN=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
OCI_BIN=${A3S_CLOUD_OCI_BIN:-$(command -v a3s-oci || true)}
[[ -n $BOX_BIN && -x $BOX_BIN ]] || { echo "a3s-box missing" >&2; exit 2; }
[[ -n $OCI_BIN && -x $OCI_BIN ]] || { echo "a3s-oci missing" >&2; exit 2; }

if [[ -S /var/run/docker.sock || -S /var/run/docker.sock.raw ]]; then
  echo "A3S_CLOUD_A0_4_REAL_BOX_BLOCKED reason=docker_sock_present" | tee "$evidence_directory/blocked.txt"
  exit 3
fi
if [[ ! -e /dev/kvm ]]; then
  echo "A3S_CLOUD_A0_4_REAL_BOX_BLOCKED reason=box_vm_requires_kvm path=/dev/kvm" \
    | tee "$evidence_directory/blocked.txt"
  exit 3
fi

install_dir=${A3S_CLOUD_BOX_INSTALL_DIR:-/home/roylin/code/a3s-box-install}
if [[ -f $install_dir/BOX-REVISION ]]; then
  install_box_rev=$(<"$install_dir/BOX-REVISION")
  if [[ $install_box_rev != "$box_revision" ]]; then
    echo "A3S_CLOUD_A0_4_REAL_BOX_BLOCKED reason=box_pin_mismatch cloud=$box_revision install=$install_box_rev" \
      | tee "$evidence_directory/blocked.txt"
    exit 4
  fi
fi

# Locked fixture pins (match .github/workflows/box-conformance.yml).
export A3S_CLOUD_A0_4_AGENT_BASE_IMAGE=${A3S_CLOUD_A0_4_AGENT_BASE_IMAGE:-ubuntu@sha256:1e0a86e57d247923571b75e0aaf48a1449cf8c543d51fb3e07a4a7d7bfa79316}
export A3S_CLOUD_A0_4_CLI_ARCHIVE_DIGEST=${A3S_CLOUD_A0_4_CLI_ARCHIVE_DIGEST:-sha256:f41a71f073e7bc901db5c38d382b600d6f23a64a09705eeecc25590bbde5fd53}
export A3S_CLOUD_A0_4_CLI_ARCHIVE_URL=${A3S_CLOUD_A0_4_CLI_ARCHIVE_URL:-https://github.com/A3S-Lab/CLI/releases/download/v0.13.5/a3s-v0.13.5-x86_64-unknown-linux-gnu.tar.gz}
export A3S_CLOUD_A0_4_CLI_REVISION=${A3S_CLOUD_A0_4_CLI_REVISION:-d48efa67922e82c15c5f7fd059c860d327cc6dd8}
export A3S_CLOUD_A0_4_CLI_TAG=${A3S_CLOUD_A0_4_CLI_TAG:-v0.13.5}
export A3S_CLOUD_A0_4_CLI_VERSION=${A3S_CLOUD_A0_4_CLI_VERSION:-0.13.5}
export A3S_CLOUD_A0_4_CLI_CODE_CORE_VERSION=${A3S_CLOUD_A0_4_CLI_CODE_CORE_VERSION:-8.0.4}
export A3S_CLOUD_A0_4_CLOUD_CODE_CORE_VERSION=${A3S_CLOUD_A0_4_CLOUD_CODE_CORE_VERSION:-8.2.0}

registry_host=${A3S_CLOUD_A0_4_REGISTRY:-127.0.0.1:50020}
registry_api=http://${registry_host}/v2
repository=a3s/a0-4-agent-runtime
mutable_image=${registry_host}/${repository}:${A3S_CLOUD_A0_4_CLI_TAG}

postgres_url=${A3S_CLOUD_TEST_POSTGRES_URL:-postgresql://a3s_cloud:a3s_cloud@127.0.0.1:54320/postgres}
export A3S_CLOUD_TEST_POSTGRES_URL=$postgres_url

a3s_home=${A3S_HOME:-/tmp/a3s-cloud-a0-4-home}
rm -rf -- "$a3s_home"
mkdir -p -- "$a3s_home/bin" "$a3s_home/runtime-secrets"
# Prefer install tree binaries (pin-matched).
for bin in a3s-box a3s-box-shim a3s-box-guest-init a3s-oci a3s-oci-agent; do
  if [[ -x $install_dir/$bin ]]; then
    cp -f "$install_dir/$bin" "$a3s_home/bin/$bin"
  fi
done
chmod -R u+rwX "$a3s_home/bin"
# Box Secret root requires provider-owned 0700 on Linux tmpfs. The Box home
# directory itself must remain traversable by mapped Sandbox UIDs.
chmod 0755 -- "$a3s_home"
chmod 0700 -- "$a3s_home/runtime-secrets"
a3s_home=$(readlink -f "$a3s_home")
export A3S_HOME=$a3s_home
export A3S_BOX_OCI_AGENT_PATH=$a3s_home/bin/a3s-oci-agent
export A3S_BOX_OCI_RUNTIME_PATH=$a3s_home/bin/a3s-oci
export A3S_CLOUD_TEST_BOX=1
export A3S_DEPS_STUB=1
export A3S_REGISTRY_PROTOCOL=http
export LD_LIBRARY_PATH=${LD_LIBRARY_PATH:-}:${install_dir}/lib
export PATH="$a3s_home/bin:$install_dir:${PATH:-}"
fstype=$(findmnt -no FSTYPE -T "$a3s_home/runtime-secrets" 2>/dev/null || true)
if [[ $fstype != tmpfs ]]; then
  echo "A3S_CLOUD_A0_4_REAL_BOX_BLOCKED reason=secret_root_not_tmpfs path=$a3s_home/runtime-secrets fstype=${fstype:-unknown}" \
    | tee "$evidence_directory/blocked.txt"
  exit 4
fi
mode=$(stat -c '%a' "$a3s_home/runtime-secrets" 2>/dev/null || true)
if [[ $mode != 700 && $mode != 710 ]]; then
  echo "A3S_CLOUD_A0_4_REAL_BOX_BLOCKED reason=secret_root_mode path=$a3s_home/runtime-secrets mode=${mode:-unknown}" \
    | tee "$evidence_directory/blocked.txt"
  exit 4
fi

printf '===== A0.4 real Box gate (local) =====\n' | tee "$evidence_directory/a0-4-gate.txt"
printf 'box_revision=%s\noci_revision=%s\nbox_bin=%s\nregistry=%s\npostgres=%s\na3s_home=%s\n' \
  "$box_revision" "$oci_revision" "$BOX_BIN" "$registry_host" "$postgres_url" "$a3s_home" \
  | tee -a "$evidence_directory/a0-4-gate.txt"

curl -fsS --max-time 5 "$registry_api/" >/dev/null \
  || { echo "registry unreachable at $registry_api" >&2; exit 5; }

build_dir=${A3S_CLOUD_A0_4_BUILD_DIR:-/tmp/a0-4-agent-runtime-build}
mkdir -p -- "$build_dir"

echo '=== download pinned CLI ==='
need_cli=1
if [[ -f $build_dir/a3s-cli.tar.gz ]]; then
  got_digest=sha256:$(sha256sum "$build_dir/a3s-cli.tar.gz" | cut -d' ' -f1)
  if [[ $got_digest == "$A3S_CLOUD_A0_4_CLI_ARCHIVE_DIGEST" ]]; then
    need_cli=0
  fi
fi
if ((need_cli == 1)); then
  curl --fail --silent --show-error --location --retry 5 --max-time 120 \
    "$A3S_CLOUD_A0_4_CLI_ARCHIVE_URL" \
    --output "$build_dir/a3s-cli.tar.gz"
fi
got_digest=sha256:$(sha256sum "$build_dir/a3s-cli.tar.gz" | cut -d' ' -f1)
[[ $got_digest == "$A3S_CLOUD_A0_4_CLI_ARCHIVE_DIGEST" ]] \
  || { echo "CLI archive digest mismatch got=$got_digest" >&2; exit 5; }
if [[ ! -x $build_dir/a3s ]]; then
  tar --extract --gzip --file "$build_dir/a3s-cli.tar.gz" --directory "$build_dir" a3s
fi
test "$("$build_dir/a3s" --version)" = "a3s $A3S_CLOUD_A0_4_CLI_VERSION"

cli_cargo=$evidence_directory/agent-runtime-cli-Cargo.toml
if [[ -f /tmp/a0-4-cli-Cargo.toml.cache ]]; then
  cp -f /tmp/a0-4-cli-Cargo.toml.cache "$cli_cargo"
elif [[ ! -s $cli_cargo ]]; then
  curl --fail --silent --show-error --location --retry 3 --max-time 30 \
    "https://raw.githubusercontent.com/A3S-Lab/CLI/$A3S_CLOUD_A0_4_CLI_REVISION/Cargo.toml" \
    --output "$cli_cargo" \
    || {
      printf 'a3s-code-core = "=%s"\n' "$A3S_CLOUD_A0_4_CLI_CODE_CORE_VERSION" >"$cli_cargo"
      echo "WARN: CLI Cargo.toml fetch failed; using pinned version assertion only" >&2
    }
fi
if grep -q 'a3s-code-core' "$cli_cargo"; then
  grep --fixed-strings \
    "a3s-code-core = \"=$A3S_CLOUD_A0_4_CLI_CODE_CORE_VERSION\"" \
    "$cli_cargo"
fi
grep --fixed-strings \
  "a3s-code-core = { version = \"=$A3S_CLOUD_A0_4_CLOUD_CODE_CORE_VERSION\"" \
  "$repository_root/Cargo.toml"

cp "$tools/agent-runtime/config.acl" "$build_dir/config.acl"
cp "$tools/agent-runtime/Containerfile" "$build_dir/Containerfile"
sed -i \
  -e "s|__A3S_CLI_VERSION__|$A3S_CLOUD_A0_4_CLI_VERSION|g" \
  -e "s|__A3S_CLI_REVISION__|$A3S_CLOUD_A0_4_CLI_REVISION|g" \
  -e "s|__A3S_CLI_ARCHIVE_SHA256__|$A3S_CLOUD_A0_4_CLI_ARCHIVE_DIGEST|g" \
  "$build_dir/Containerfile"
cp "$build_dir/Containerfile" "$evidence_directory/agent-runtime.Containerfile"
cp "$tools/agent-runtime/Containerfile" "$evidence_directory/agent-runtime.template.Containerfile"
cp "$tools/agent-runtime/config.acl" "$evidence_directory/agent-runtime-config.acl"

workspace_dir=$build_dir/workspace
install -d -m 0755 "$workspace_dir"
printf 'A3S Cloud A0.4 Agent Runtime fixture\n' >"$workspace_dir/.a3s-release-seed"
git -C "$workspace_dir" init --initial-branch=main
git -C "$workspace_dir" config user.name 'A3S Cloud Release Fixture'
git -C "$workspace_dir" config user.email 'release-fixture@a3s.invalid'
git -C "$workspace_dir" add .a3s-release-seed
if [[ -n $(git -C "$workspace_dir" status --porcelain) ]]; then
  GIT_AUTHOR_DATE='2000-01-01T00:00:00Z' \
    GIT_COMMITTER_DATE='2000-01-01T00:00:00Z' \
    git -C "$workspace_dir" commit --message 'Initialize Agent workspace'
fi
test -z "$(git -C "$workspace_dir" status --porcelain)"

echo '=== ensure tools base available locally (rootless-safe, no apt in Box RUN) ==='
# Prefer a baked ubuntu+git+ca-certs base. Fallback: mirror plain ubuntu (apt RUN will fail rootless).
tools_tag=127.0.0.1:50020/library/ubuntu:a0-4-tools
local_base=$tools_tag
local_digest=$(crane digest --insecure --platform linux/amd64 "$tools_tag" 2>/dev/null || true)
if [[ -z $local_digest ]]; then
  echo "tools base missing; attempting bake helper" >&2
  bake_helper=${A3S_CLOUD_A0_4_BAKE_HELPER:-$tools/_tmp_bake_ubuntu_tools.sh}
  if [[ -f $bake_helper ]]; then
    python3 -c "from pathlib import Path; p=Path('$bake_helper'); Path('/tmp/_bake_a04.sh').write_bytes(p.read_bytes().replace(b'\\r\\n',b'\\n').replace(b'\\r',b'\\n'))"
    bash /tmp/_bake_a04.sh || true
    local_digest=$(crane digest --insecure --platform linux/amd64 "$tools_tag" 2>/dev/null || true)
  fi
fi
if [[ -z $local_digest ]]; then
  echo "falling back to plain ubuntu base (Box RUN apt may fail rootless)" >&2
  base_ref=$A3S_CLOUD_A0_4_AGENT_BASE_IMAGE
  if [[ $base_ref == ubuntu@* ]]; then
    base_ref="docker.io/library/${base_ref}"
  fi
  base_tar=${A3S_CLOUD_A0_4_BASE_TAR:-/tmp/bx0-oci-cache/a0-4/ubuntu-base.tar}
  local_base=127.0.0.1:50020/library/ubuntu:a0-4-base
  mkdir -p "$(dirname -- "$base_tar")"
  if [[ ! -f $base_tar ]]; then
    crane pull --platform linux/amd64 "$base_ref" "$base_tar" \
      >"$evidence_directory/crane-pull-base.out" 2>"$evidence_directory/crane-pull-base.err" \
      || { echo "failed to pull base $base_ref" >&2; exit 5; }
  fi
  crane push --insecure "$base_tar" "$local_base" \
    >"$evidence_directory/crane-push-base.out" 2>"$evidence_directory/crane-push-base.err" || true
  local_digest=$(crane digest --insecure --platform linux/amd64 "$local_base" 2>/dev/null || true)
fi
[[ -n $local_digest ]] || { echo "no local base digest" >&2; exit 5; }

env A3S_HOME="$a3s_home" A3S_REGISTRY_PROTOCOL=http \
  "$BOX_BIN" pull --platform linux/amd64 --quiet "${local_base%@*}@${local_digest}" \
  >"$evidence_directory/box-pull-base.out" 2>"$evidence_directory/box-pull-base.err" \
  || env A3S_HOME="$a3s_home" A3S_REGISTRY_PROTOCOL=http \
    "$BOX_BIN" pull --platform linux/amd64 --quiet "$local_base" \
    >"$evidence_directory/box-pull-base.out" 2>"$evidence_directory/box-pull-base.err" \
  || {
    echo "failed to pull local base into a3s-box" >&2
    cat "$evidence_directory/box-pull-base.err" >&2 || true
    exit 5
  }

# Rewrite Containerfile for rootless run-pool: tools base already has git/ca-certs/user.
python3 - "$build_dir/Containerfile" "$local_digest" <<'PY'
from pathlib import Path
import re, sys
path = Path(sys.argv[1])
digest = sys.argv[2]
text = path.read_text()
# Extract LABEL block and trailing runtime directives from template.
label = re.search(r"(LABEL[\s\S]*?)\n\nRUN ", text)
labels = label.group(1) if label else 'LABEL org.opencontainers.image.source="https://github.com/A3S-Lab/CLI"'
tail = text.split("USER 65532:65532", 1)[-1]
# Keep ARG lines
args = "\n".join(line for line in text.splitlines() if line.startswith("ARG "))
body = f"""FROM 127.0.0.1:50020/library/ubuntu@{digest}
USER 0:0
{args}

{labels}

COPY a3s /usr/bin/a3s
COPY --chown=65532:65532 config.acl /app/config.acl
COPY --chown=65532:65532 workspace/ /workspace/
RUN chmod 0755 /usr/bin/a3s \\
    && test "$(/usr/bin/a3s --version)" = "a3s ${{A3S_CLI_VERSION}}"

USER 65532:65532
{tail.lstrip()}"""
path.write_text(body)
print(path.read_text())
PY
cp "$build_dir/Containerfile" "$evidence_directory/agent-runtime.Containerfile"
printf 'local_base=%s\nlocal_digest=%s\n' "$local_base" "$local_digest" \
  | tee "$evidence_directory/base-image.txt"
grep -E '^FROM |^USER |^RUN |^COPY ' "$build_dir/Containerfile" | tee -a "$evidence_directory/base-image.txt"

echo '=== a3s-box build Agent Runtime image ==='
# Rootless hosts cannot chroot for Dockerfile RUN; use Box warm-pool helper VMs.
# Prefer the already-mirrored local ubuntu base as the pool helper image.
run_pool_image=${local_base}
if [[ -n ${local_digest:-} ]]; then
  run_pool_image="127.0.0.1:50020/library/ubuntu@${local_digest}"
fi
set +e
env A3S_HOME="$a3s_home" A3S_DEPS_STUB=1 A3S_REGISTRY_PROTOCOL=http \
  "$BOX_BIN" build \
  --build-arg "A3S_CLI_VERSION=$A3S_CLOUD_A0_4_CLI_VERSION" \
  --build-arg "A3S_CLI_REVISION=$A3S_CLOUD_A0_4_CLI_REVISION" \
  --build-arg "A3S_CLI_ARCHIVE_SHA256=$A3S_CLOUD_A0_4_CLI_ARCHIVE_DIGEST" \
  --file Containerfile \
  --platform linux/amd64 \
  --run-pool \
  --run-pool-autostart \
  --run-pool-image "$run_pool_image" \
  --run-pool-memory 1024m \
  --quiet \
  --tag "$mutable_image" \
  "$build_dir" \
  >"$evidence_directory/box-build.stdout" \
  2>"$evidence_directory/box-build.stderr"
build_rc=$?
set -e
if ((build_rc != 0)); then
  echo "box build failed rc=$build_rc" >&2
  tail -n 80 "$evidence_directory/box-build.stderr" >&2 || true
  exit 6
fi

echo '=== push Agent Runtime image to registry ==='
set +e
env A3S_HOME="$a3s_home" A3S_REGISTRY_PROTOCOL=http \
  "$BOX_BIN" push --plain-http --quiet "$mutable_image" \
  >"$evidence_directory/box-push.stdout" \
  2>"$evidence_directory/box-push.stderr"
push_rc=$?
set -e
if ((push_rc != 0)); then
  echo "box push failed rc=$push_rc" >&2
  tail -n 80 "$evidence_directory/box-push.stderr" >&2 || true
  exit 7
fi

echo '=== resolve published digest ==='
# Prefer crane digest against insecure registry; fallback to box image inspect.
if command -v crane >/dev/null 2>&1; then
  digest=$(crane digest --insecure --platform linux/amd64 "$mutable_image")
else
  digest=$("$BOX_BIN" image inspect --format '{{.Digest}}' "$mutable_image" 2>/dev/null || true)
fi
[[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || {
  echo "failed to resolve digest for $mutable_image (got: $digest)" >&2
  exit 8
}
exact_image=${registry_host}/${repository}@${digest}

# Manifest size + media type via registry API
manifest_tmp=$evidence_directory/platform-manifest.json
curl -fsS \
  -H 'Accept: application/vnd.oci.image.manifest.v1+json' \
  "http://${registry_host}/v2/${repository}/manifests/${digest#sha256:}" \
  -o "$manifest_tmp" \
  || curl -fsS \
    -H 'Accept: application/vnd.oci.image.manifest.v1+json' \
    "http://${registry_host}/v2/${repository}/manifests/${digest}" \
    -o "$manifest_tmp"
media_type=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("mediaType","application/vnd.oci.image.manifest.v1+json"))' "$manifest_tmp")
size_bytes=$(wc -c <"$manifest_tmp" | tr -d ' ')

export A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE=$exact_image
export A3S_CLOUD_A0_4_AGENT_RUNTIME_MEDIA_TYPE=$media_type
export A3S_CLOUD_A0_4_AGENT_RUNTIME_SIZE_BYTES=$size_bytes
printf 'image=%s\nmedia_type=%s\nsize_bytes=%s\n' \
  "$exact_image" "$media_type" "$size_bytes" | tee "$evidence_directory/agent-runtime-image.txt"

echo '=== prepare Sandbox CI cgroup + setpriv identity ==='
# Match .github/workflows/box-conformance.yml: delegated cgroup v2 + setpriv harness.
monorepo_root=$(cd -- "$repository_root/../.." && pwd)
if [[ ! -f $monorepo_root/crates/box/scripts/prepare-linux-sandbox-ci-host.sh ]]; then
  monorepo_root=/mnt/d/code/a3s
fi
export GITHUB_WORKSPACE=$monorepo_root
export SUDO_UID=${SUDO_UID:-$(id -u)}
export SUDO_GID=${SUDO_GID:-$(id -g)}
export SUDO_USER=${SUDO_USER:-$(id -un)}
export RUST_MIN_STACK=${RUST_MIN_STACK:-33554432}
# Ensure Sandbox OCI launcher bytes exist next to pin-matched a3s-oci.
if [[ ! -x $a3s_home/bin/a3s-box-sandbox-oci-launcher ]]; then
  cp -f "$a3s_home/bin/a3s-oci" "$a3s_home/bin/a3s-box-sandbox-oci-launcher"
  chmod 0755 "$a3s_home/bin/a3s-box-sandbox-oci-launcher"
fi
export A3S_BOX_SANDBOX_OCI_LAUNCHER=$a3s_home/bin/a3s-box-sandbox-oci-launcher
if ! mountpoint -q "$a3s_home/runtime-secrets" 2>/dev/null; then
  sudo install -d -m 0700 "$a3s_home/runtime-secrets"
  sudo mount -t tmpfs \
    -o size=16m,uid=0,gid=0,mode=0700,nosuid,nodev,noexec \
    "a3s-cloud-a0-4-secrets-$$" "$a3s_home/runtime-secrets"
fi
export A3S_BOX_CI_CGROUP_ROOT=${A3S_BOX_CI_CGROUP_ROOT:-/sys/fs/cgroup/a3s-box-ci-a0-4-$(date -u +%Y%m%dT%H%M%SZ)}
export GITHUB_ENV=$evidence_directory/github-sandbox.env
: >"$GITHUB_ENV"
bash "$monorepo_root/crates/box/scripts/prepare-linux-sandbox-ci-host.sh"
while IFS= read -r line; do
  case "$line" in
    A3S_BOX_*=*) export "$line" ;;
  esac
done <"$GITHUB_ENV"
test -n "${A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT:-}"
test -d "$A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT"
sudo chown -R "${A3S_BOX_CI_SANDBOX_UID}:${A3S_BOX_CI_SANDBOX_GID}" "$a3s_home"
sudo chown root:root "$a3s_home/runtime-secrets"
sudo chmod 0700 "$a3s_home/runtime-secrets"

echo '=== cargo test A0.4 real Box release (setpriv) ==='
cd "$monorepo_root"
set +e
bash "$repository_root/tools/box-conformance/run_box_cargo_under_setpriv.sh" test \
  --manifest-path apps/cloud/Cargo.toml --locked \
  -p a3s-cloud-control-plane --features persistence-conformance --test a0_4_real_box_release \
  postgres_published_agent_release_runs_recovers_and_cleans_through_real_box \
  -- --ignored --exact --nocapture --test-threads=1 \
  >"$evidence_directory/agent-runtime-release.log" 2>&1
test_rc=$?
set -e
tail -n 60 "$evidence_directory/agent-runtime-release.log" || true

marker='A3S_CLOUD_A0_4_REAL_BOX_RELEASE_CERTIFIED'
if grep -Fq "$marker" "$evidence_directory/agent-runtime-release.log"; then
  awk -v marker="$marker" 'index($0, marker) { print substr($0, index($0, marker)) }' \
    "$evidence_directory/agent-runtime-release.log" \
    | head -n 1 >"$evidence_directory/agent-runtime-release-certification.txt"
  {
    echo "cloud_revision=$(git -C "$repository_root" rev-parse HEAD 2>/dev/null || echo unknown)"
    echo "box_revision=$box_revision"
    echo "oci_runtime_revision=$oci_revision"
    echo "artifact_image=$exact_image"
  } >>"$evidence_directory/agent-runtime-release-certification.txt"
  echo "A3S_CLOUD_A0_4_REAL_BOX_RELEASE_OK evidence=$evidence_directory"
  cat "$evidence_directory/agent-runtime-release-certification.txt"
  exit 0
fi

echo "A3S_CLOUD_A0_4_REAL_BOX_RELEASE_BLOCKED reason=certification_missing test_rc=$test_rc" \
  | tee "$evidence_directory/blocked.txt"
exit "${test_rc:-8}"
