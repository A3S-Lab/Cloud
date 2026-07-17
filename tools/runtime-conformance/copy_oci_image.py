#!/usr/bin/env python3
"""Copy a digest-pinned OCI image between registries without changing bytes."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any


ACCEPT_MANIFESTS = ", ".join(
    [
        "application/vnd.oci.image.index.v1+json",
        "application/vnd.docker.distribution.manifest.list.v2+json",
        "application/vnd.oci.image.manifest.v1+json",
        "application/vnd.docker.distribution.manifest.v2+json",
    ]
)
DIGEST_PATTERN = re.compile(r"sha256:[0-9a-f]{64}\Z")
REPOSITORY_PATTERN = re.compile(
    r"[a-z0-9]+(?:[._-][a-z0-9]+)*(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*)*\Z"
)
REFERENCE_PATTERN = re.compile(r"[A-Za-z0-9_][A-Za-z0-9._-]{0,127}\Z")


@dataclass(frozen=True)
class CopyConfig:
    source: str
    token_url: str
    service: str
    repository: str
    digest: str
    target: str
    tag: str


class OciRegistryCopier:
    def __init__(self, config: CopyConfig) -> None:
        self.config = config
        self.token = self._read_token()
        self.copied_manifests: set[str] = set()
        self.copied_blobs: set[str] = set()
        self.blob_bytes = 0

    def copy(self) -> None:
        self._copy_manifest(self.config.digest)
        root, media_type = self._source_manifest(self.config.digest)
        self._put_manifest(self.config.tag, root, media_type)
        returned = self._target_manifest_digest(self.config.digest)
        if returned != self.config.digest:
            raise RuntimeError(
                "target root digest mismatch: "
                f"expected {self.config.digest}, got {returned!r}"
            )
        print(
            "OCI_COPY_PASS "
            f"digest={returned} manifests={len(self.copied_manifests)} "
            f"blobs={len(self.copied_blobs)} blob_bytes={self.blob_bytes}"
        )

    def _read_token(self) -> str:
        query = urllib.parse.urlencode(
            {
                "service": self.config.service,
                "scope": f"repository:{self.config.repository}:pull",
            }
        )
        separator = "&" if urllib.parse.urlparse(self.config.token_url).query else "?"
        with urllib.request.urlopen(
            f"{self.config.token_url}{separator}{query}", timeout=30
        ) as response:
            payload: dict[str, Any] = json.load(response)
        token = payload.get("token") or payload.get("access_token")
        if not isinstance(token, str) or not token:
            raise RuntimeError("source token response omitted a bearer token")
        return token

    def _source_request(
        self, path: str, *, accept: str | None = None
    ) -> urllib.response.addinfourl:
        headers = {"Authorization": f"Bearer {self.token}"}
        if accept is not None:
            headers["Accept"] = accept
        request = urllib.request.Request(
            f"{self.config.source}{path}", headers=headers
        )
        return urllib.request.urlopen(request, timeout=120)

    def _source_blob(self, digest: str) -> bytes:
        result = subprocess.run(
            [
                "curl",
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-time",
                "120",
                "--header",
                f"Authorization: Bearer {self.token}",
                (
                    f"{self.config.source}/v2/{self.config.repository}"
                    f"/blobs/{digest}"
                ),
            ],
            check=True,
            stdout=subprocess.PIPE,
            timeout=130,
        )
        return result.stdout

    def _source_manifest(self, digest: str) -> tuple[bytes, str]:
        path = f"/v2/{self.config.repository}/manifests/{digest}"
        with self._source_request(path, accept=ACCEPT_MANIFESTS) as response:
            data = response.read()
            media_type = response.headers.get("Content-Type", "").split(";", 1)[0]
        actual = digest_bytes(data)
        if actual != digest:
            raise RuntimeError(
                f"source manifest digest mismatch: expected {digest}, got {actual}"
            )
        if not media_type:
            raise RuntimeError(f"source manifest {digest} omitted its media type")
        return data, media_type

    def _target_blob_exists(self, digest: str) -> bool:
        request = urllib.request.Request(
            f"{self.config.target}/v2/{self.config.repository}/blobs/{digest}",
            method="HEAD",
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status == 200
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return False
            raise

    def _copy_blob(self, digest: str, expected_size: int | None) -> None:
        if digest in self.copied_blobs:
            return
        if self._target_blob_exists(digest):
            self.copied_blobs.add(digest)
            return
        data = self._source_blob(digest)
        actual = digest_bytes(data)
        if actual != digest:
            raise RuntimeError(
                f"source blob digest mismatch: expected {digest}, got {actual}"
            )
        if expected_size is not None and len(data) != expected_size:
            raise RuntimeError(
                f"source blob size mismatch for {digest}: "
                f"expected {expected_size}, got {len(data)}"
            )

        start = urllib.request.Request(
            f"{self.config.target}/v2/{self.config.repository}/blobs/uploads/",
            data=b"",
            method="POST",
        )
        with urllib.request.urlopen(start, timeout=30) as response:
            location = response.headers.get("Location")
        if not location:
            raise RuntimeError("target blob upload omitted its Location")
        upload_url = urllib.parse.urljoin(self.config.target, location)
        separator = "&" if urllib.parse.urlparse(upload_url).query else "?"
        upload_url = (
            f"{upload_url}{separator}"
            f"{urllib.parse.urlencode({'digest': digest})}"
        )
        upload = urllib.request.Request(
            upload_url,
            data=data,
            method="PUT",
            headers={"Content-Type": "application/octet-stream"},
        )
        with urllib.request.urlopen(upload, timeout=120) as response:
            if response.status != 201:
                raise RuntimeError(
                    f"target blob upload returned HTTP {response.status}"
                )
        self.copied_blobs.add(digest)
        self.blob_bytes += len(data)

    def _put_manifest(self, reference: str, data: bytes, media_type: str) -> None:
        request = urllib.request.Request(
            (
                f"{self.config.target}/v2/{self.config.repository}"
                f"/manifests/{reference}"
            ),
            data=data,
            method="PUT",
            headers={"Content-Type": media_type},
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            returned = response.headers.get("Docker-Content-Digest")
            if response.status != 201:
                raise RuntimeError(
                    f"target manifest upload returned HTTP {response.status}"
                )
        actual = digest_bytes(data)
        if returned is not None and returned != actual:
            raise RuntimeError(
                f"target manifest digest mismatch: expected {actual}, got {returned}"
            )

    def _copy_manifest(self, digest: str) -> None:
        if digest in self.copied_manifests:
            return
        data, media_type = self._source_manifest(digest)
        document: dict[str, Any] = json.loads(data)
        if "manifests" in document:
            for descriptor in document["manifests"]:
                self._copy_manifest(descriptor["digest"])
        else:
            descriptors: list[dict[str, Any]] = []
            if document.get("config") is not None:
                descriptors.append(document["config"])
            descriptors.extend(document.get("layers", []))
            for descriptor in descriptors:
                self._copy_blob(descriptor["digest"], descriptor.get("size"))
        self._put_manifest(digest, data, media_type)
        self.copied_manifests.add(digest)

    def _target_manifest_digest(self, digest: str) -> str | None:
        request = urllib.request.Request(
            (
                f"{self.config.target}/v2/{self.config.repository}"
                f"/manifests/{digest}"
            ),
            method="HEAD",
            headers={"Accept": ACCEPT_MANIFESTS},
        )
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.headers.get("Docker-Content-Digest")


def digest_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def validated_url(
    value: str,
    label: str,
    schemes: set[str],
    *,
    allow_path: bool = False,
) -> str:
    parsed = urllib.parse.urlparse(value)
    invalid_path = not allow_path and parsed.path not in ("", "/")
    if parsed.scheme not in schemes or not parsed.netloc or invalid_path:
        choices = ", ".join(sorted(schemes))
        raise argparse.ArgumentTypeError(
            f"{label} must be an origin using one of: {choices}"
        )
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise argparse.ArgumentTypeError(f"{label} must not contain credentials or a query")
    return value.rstrip("/")


def parse_args() -> CopyConfig:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--token-url", required=True)
    parser.add_argument("--service", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--digest", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--tag", default="conformance")
    args = parser.parse_args()

    source = validated_url(args.source, "source", {"https"})
    token_url = validated_url(
        args.token_url, "token URL", {"https"}, allow_path=True
    )
    target = validated_url(args.target, "target", {"http", "https"})
    if not REPOSITORY_PATTERN.fullmatch(args.repository):
        parser.error("repository is not a valid lowercase OCI repository path")
    if not DIGEST_PATTERN.fullmatch(args.digest):
        parser.error("digest must be a lowercase sha256 digest")
    if not REFERENCE_PATTERN.fullmatch(args.tag):
        parser.error("tag is not a valid OCI tag")
    if not args.service or any(character.isspace() for character in args.service):
        parser.error("service must be a nonempty token")
    return CopyConfig(
        source=source,
        token_url=token_url,
        service=args.service,
        repository=args.repository,
        digest=args.digest,
        target=target,
        tag=args.tag,
    )


def main() -> None:
    OciRegistryCopier(parse_args()).copy()


if __name__ == "__main__":
    main()
