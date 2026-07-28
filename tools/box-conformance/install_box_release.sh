#!/usr/bin/env bash
set -euo pipefail

readonly BOX_VERSION=3.1.0
readonly BOX_ARCHIVE=a3s-box-v3.1.0-linux-x86_64.tar.gz
readonly BOX_ARCHIVE_SHA256=d1aa83dc0111f8982a8ac984064fd4e8cf553deb87a94f28ad85b9f1da9af530
readonly OCI_RUNTIME_VERSION=0.2.0
readonly OCI_RUNTIME_ARCHIVE=a3s-oci-runtime-v0.2.0-linux-x86_64.tar.gz
readonly OCI_RUNTIME_ARCHIVE_SHA256=b50e9f3f653b8c23d0e54a1096d9e28cc6f2a85a06286a23f82069a338ca2504
readonly OCI_RUNTIME_REVISION=503625b176de7f22b2e31c782b82e97897e8c368

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

oci_archive="$install_root/$OCI_RUNTIME_ARCHIVE"
curl --fail --location --silent --show-error \
  "https://github.com/A3S-Lab/OCI-Runtime/releases/download/v$OCI_RUNTIME_VERSION/$OCI_RUNTIME_ARCHIVE" \
  --output "$oci_archive"
printf '%s  %s\n' "$OCI_RUNTIME_ARCHIVE_SHA256" "$oci_archive" \
  | sha256sum --check --strict
oci_extract_root="$install_root/.oci-runtime"
install -d -m 0755 "$oci_extract_root"
tar --extract --gzip --file "$oci_archive" --directory "$oci_extract_root" \
  --strip-components=1
install -m 0755 "$oci_extract_root/a3s-oci" "$install_root/a3s-oci"
install -m 0755 "$oci_extract_root/a3s-oci-agent" "$install_root/a3s-oci-agent"
rm -rf "$oci_extract_root"
rm -f "$oci_archive"
printf '%s\n' "$OCI_RUNTIME_REVISION" >"$install_root/OCI-RUNTIME-REVISION"

for binary in a3s-box a3s-box-guest-init a3s-box-shim a3s-oci a3s-oci-agent; do
  test -x "$install_root/$binary"
done
grep --fixed-strings --line-regexp "$OCI_RUNTIME_REVISION" \
  "$install_root/OCI-RUNTIME-REVISION"
LD_LIBRARY_PATH="$install_root/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  "$install_root/a3s-box" --version | grep --fixed-strings "a3s-box $BOX_VERSION"
