#!/usr/bin/env bash
set -euo pipefail

readonly BOX_VERSION=3.1.0
readonly BOX_ARCHIVE=a3s-box-v3.1.0-linux-x86_64.tar.gz
readonly BOX_ARCHIVE_SHA256=d1aa83dc0111f8982a8ac984064fd4e8cf553deb87a94f28ad85b9f1da9af530
readonly BOX_OCI_RUNTIME_REVISION=0f0face67c282cf535e352b4fe83af02055b7e08

install_root="${1:-}"
if [[ -z $install_root || $install_root != /* ]]; then
  printf 'usage: %s ABSOLUTE_EMPTY_INSTALL_DIRECTORY\n' "$0" >&2
  exit 2
fi
if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
  printf '%s\n' 'the pinned Box fixture release requires Linux x86_64' >&2
  exit 1
fi
if [[ -e $install_root ]] &&
  [[ ! -d $install_root || -n $(find "$install_root" -mindepth 1 -maxdepth 1 -print -quit) ]]; then
  printf '%s\n' 'Box install directory must be absent or empty' >&2
  exit 1
fi

install -d -m 0755 "$install_root"
archive="$install_root/$BOX_ARCHIVE"
curl --fail --location --silent --show-error \
  "https://github.com/A3S-Lab/Box/releases/download/v$BOX_VERSION/$BOX_ARCHIVE" \
  --output "$archive"
printf '%s  %s\n' "$BOX_ARCHIVE_SHA256" "$archive" | sha256sum --check --strict
tar --extract --gzip --file "$archive" --directory "$install_root" --strip-components=1
rm -f "$archive"

for binary in a3s-box a3s-box-guest-init a3s-box-shim a3s-oci a3s-oci-agent; do
  test -x "$install_root/$binary"
done
grep --fixed-strings --line-regexp "$BOX_OCI_RUNTIME_REVISION" \
  "$install_root/OCI-RUNTIME-REVISION"
LD_LIBRARY_PATH="$install_root/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  "$install_root/a3s-box" --version | grep --fixed-strings "a3s-box $BOX_VERSION"
