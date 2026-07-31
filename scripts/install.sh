#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
cargo_root="${CARGO_HOME:-${HOME}/.cargo}"
install_dir="${A3S_WORKFLOW_INSTALL_DIR:-${cargo_root}/bin}"
codex_root="${CODEX_HOME:-${HOME}/.codex}"
skill_dir="${A3S_WORKFLOW_SKILL_DIR:-${codex_root}/skills/a3s-workflow}"
install_cli=1
install_skill=1
deploy=1
dry_run=0

usage() {
  cat <<'EOF'
Install the A3S Workflow coding-agent CLI and skill, then deploy the local stack.

Usage: scripts/install.sh [options]

Options:
  --install-dir PATH  CLI destination (default ${CARGO_HOME:-~/.cargo}/bin)
  --skill-dir PATH    Skill destination (default ${CODEX_HOME:-~/.codex}/skills/a3s-workflow)
  --no-cli            Do not build or install the CLI
  --no-skill          Do not install the coding-agent skill
  --no-deploy         Do not run Docker Compose
  --dry-run           Print actions without changing the machine
  -h, --help          Show this help
EOF
}

while (($#)); do
  case "$1" in
    --install-dir)
      [[ $# -ge 2 ]] || { echo "--install-dir requires a path" >&2; exit 2; }
      install_dir="$2"
      shift 2
      ;;
    --skill-dir)
      [[ $# -ge 2 ]] || { echo "--skill-dir requires a path" >&2; exit 2; }
      skill_dir="$2"
      shift 2
      ;;
    --no-cli) install_cli=0; shift ;;
    --no-skill) install_skill=0; shift ;;
    --no-deploy) deploy=0; shift ;;
    --dry-run) dry_run=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ -f "${repo_root}/Cargo.toml" && -f "${repo_root}/compose.yaml" ]] || {
  echo "installer must run from an A3S Workflow checkout" >&2
  exit 1
}

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  if ((dry_run == 0)); then
    "$@"
  fi
}

require_command() {
  if ((dry_run == 0)) && ! command -v "$1" >/dev/null 2>&1; then
    echo "required command not found: $1" >&2
    exit 1
  fi
}

if ((install_cli)); then
  require_command cargo
  run cargo build --manifest-path "${repo_root}/Cargo.toml" --package a3s-workflow-cli --release --locked
  run mkdir -p "${install_dir}"
  run install -m 0755 "${repo_root}/target/release/a3s-workflow" "${install_dir}/a3s-workflow"
fi

if ((install_skill)); then
  skill_source="${repo_root}/skills/a3s-workflow"
  [[ -f "${skill_source}/SKILL.md" ]] || { echo "missing skill source: ${skill_source}" >&2; exit 1; }
  if [[ -e "${skill_dir}" ]]; then
    backup="${skill_dir}.backup-$(date -u +%Y%m%d%H%M%S)"
    run mv "${skill_dir}" "${backup}"
  fi
  run mkdir -p "$(dirname -- "${skill_dir}")"
  run cp -R "${skill_source}" "${skill_dir}"
fi

if ((deploy)); then
  require_command docker
  run docker compose --project-directory "${repo_root}" -f "${repo_root}/compose.yaml" up --build --detach
  if ((dry_run == 0)); then
    require_command curl
    deadline=$((SECONDS + 120))
    until curl --fail --silent --show-error http://127.0.0.1:8080/api/health >/dev/null; do
      if ((SECONDS >= deadline)); then
        echo "A3S Workflow API did not become healthy within 120 seconds" >&2
        exit 1
      fi
      sleep 2
    done
  fi
fi

echo "A3S Workflow installation completed."
if ((install_cli)); then
  echo "CLI: ${install_dir}/a3s-workflow"
fi
if ((install_skill)); then
  echo "Skill: ${skill_dir}"
fi
if ((deploy)); then
  echo "Studio: http://127.0.0.1:3000"
  echo "API: http://127.0.0.1:8080"
fi
