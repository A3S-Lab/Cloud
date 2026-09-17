# shellcheck shell=bash
# Shared fail-closed Docker refuse for BX0.software LIVE / CREATE / EXIT.
#
# Always refuses DOCKER_HOST and the canonical /var/run/docker.sock so pointing
# A3S_CLOUD_BX0_DOCKER_SOCK_PATH at a missing path cannot bypass a dirty host.
# Also refuses an explicitly configured sock path when that path exists (CI
# injects a fake sock on Docker-free runners).
#
# Caller must define fail_blocked <reason> [detail...] OR set
# BX0_DOCKER_REFUSE_MODE=print (prints reason lines to stderr and returns 2).

bx0_refuse_docker_host() {
  local canonical=/var/run/docker.sock
  local override=${A3S_CLOUD_BX0_DOCKER_SOCK_PATH:-}
  local emit

  emit() {
    local reason=$1
    shift || true
    if declare -F fail_blocked >/dev/null 2>&1; then
      fail_blocked "$reason" "$@"
    else
      printf '%s\n' "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=$reason $*" >&2
      return 2
    fi
  }

  if [[ -n ${DOCKER_HOST:-} ]]; then
    emit docker_host_set || return 2
  fi
  if [[ -e $canonical || -S $canonical ]]; then
    emit docker_sock_present "path=$canonical" || return 2
  fi
  if [[ -n $override && ( -e $override || -S $override ) ]]; then
    emit docker_sock_present "path=$override" || return 2
  fi
  return 0
}
