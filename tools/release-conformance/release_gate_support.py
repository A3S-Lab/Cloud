#!/usr/bin/env python3
"""Shared primitives for the clean-host E0 release scenario."""

from __future__ import annotations

import http.client
import json
import os
import pathlib
import re
import socket
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from typing import Any, Callable, Dict, Iterable, List, Optional, Sequence, Tuple


DIGEST_PATTERN = re.compile(r"sha256:[0-9a-f]{64}\Z")
SAFE_RELEASE_PATTERN = re.compile(r"[a-z0-9-]{1,64}\Z")
SAFE_MARKER_PATTERN = re.compile(r"[A-Z0-9_]{1,128}\Z")
BOOTSTRAP_TOKEN_ENV = "A3S_CLOUD_BOOTSTRAP_TOKEN"
ADMIN_TOKEN_ENV = "A3S_CLOUD_ADMIN_TOKEN"
ENROLLMENT_TOKEN_ENV = "A3S_CLOUD_ENROLLMENT_TOKEN"


class GateError(RuntimeError):
    """The release scenario violated an acceptance invariant."""


class FatalGateError(GateError):
    """A terminal state makes further polling invalid."""


def required_environment(name: str) -> str:
    value = os.environ.get(name, "")
    if not value:
        raise GateError(f"required environment variable is missing: {name}")
    return value


def field(document: Dict[str, Any], name: str, expected_type: type) -> Any:
    value = document.get(name)
    if not isinstance(value, expected_type) or isinstance(value, bool):
        raise GateError(f"response field {name!r} has the wrong type")
    return value


def uuid_field(document: Dict[str, Any], name: str) -> str:
    value = field(document, name, str)
    try:
        parsed = uuid.UUID(value)
    except ValueError as error:
        raise GateError(f"response field {name!r} is not a UUID") from error
    return str(parsed)


def load_context(path: pathlib.Path) -> Dict[str, str]:
    try:
        context = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise GateError(f"could not load release context: {error}") from error
    if not isinstance(context, dict):
        raise GateError("release context is not an object")
    result = {}
    for name in ("organizationId", "projectId", "environmentId"):
        result[name] = uuid_field(context, name)
    return result


def validate_digest(value: str) -> str:
    if DIGEST_PATTERN.fullmatch(value) is None:
        raise ValueError("artifact digest must be lowercase sha256")
    return value


def service_template(
    artifact_uri: str,
    digest: str,
    release: str,
    log_marker: str,
) -> Dict[str, Any]:
    validate_digest(digest)
    if not artifact_uri.endswith(f"@{digest}") or not artifact_uri.startswith("oci://"):
        raise ValueError("artifact URI must bind the exact digest")
    if SAFE_RELEASE_PATTERN.fullmatch(release) is None:
        raise ValueError("release marker is invalid")
    if SAFE_MARKER_PATTERN.fullmatch(log_marker) is None:
        raise ValueError("log marker is invalid")
    command = (
        "mkdir -p /www && "
        f"printf '%s\\n' '{log_marker}' && "
        f"printf '%s\\n' '{release}' >/www/index.html && "
        "exec httpd -f -p 8080 -h /www"
    )
    return {
        "artifact": {
            "uri": artifact_uri,
            "expectedDigest": digest,
        },
        "process": {
            "command": ["/bin/sh"],
            "args": ["-c", command],
            "workingDirectory": None,
            "environment": {},
        },
        "secrets": [],
        "resources": {
            "cpuMillis": 100,
            "memoryBytes": 64 * 1024 * 1024,
            "pids": 32,
            "ephemeralStorageBytes": None,
        },
        "ports": [{"name": "http", "containerPort": 8080}],
        "health": {
            "portName": "http",
            "path": "/",
            "intervalMs": 250,
            "timeoutMs": 200,
            "healthyThreshold": 2,
            "unhealthyThreshold": 2,
            "stabilizationWindowMs": 250,
        },
    }


def parse_http_response(raw: bytes) -> Tuple[int, Dict[str, str], bytes]:
    header_block, separator, body = raw.partition(b"\r\n\r\n")
    if not separator:
        raise GateError("Gateway response omitted its header terminator")
    lines = header_block.split(b"\r\n")
    if not lines:
        raise GateError("Gateway response omitted its status line")
    status_parts = lines[0].decode("ascii", "strict").split(" ", 2)
    if len(status_parts) < 2 or not status_parts[1].isdigit():
        raise GateError("Gateway response status line is invalid")
    headers: Dict[str, str] = {}
    for encoded in lines[1:]:
        name, delimiter, value = encoded.partition(b":")
        if not delimiter:
            raise GateError("Gateway response contains an invalid header")
        headers[name.decode("ascii", "strict").lower()] = value.decode(
            "latin-1", "strict"
        ).strip()
    if "chunked" in headers.get("transfer-encoding", "").lower():
        body = _decode_chunked(body)
    elif "content-length" in headers:
        try:
            expected = int(headers["content-length"])
        except ValueError as error:
            raise GateError("Gateway response content length is invalid") from error
        if len(body) < expected:
            raise GateError("Gateway response body is truncated")
        body = body[:expected]
    return int(status_parts[1]), headers, body


def _decode_chunked(encoded: bytes) -> bytes:
    decoded = bytearray()
    remaining = encoded
    while True:
        size_line, separator, remaining = remaining.partition(b"\r\n")
        if not separator:
            raise GateError("chunked Gateway response omitted a size delimiter")
        size_text = size_line.split(b";", 1)[0]
        try:
            size = int(size_text, 16)
        except ValueError as error:
            raise GateError("chunked Gateway response has an invalid size") from error
        if size == 0:
            return bytes(decoded)
        if len(remaining) < size + 2 or remaining[size : size + 2] != b"\r\n":
            raise GateError("chunked Gateway response is truncated")
        decoded.extend(remaining[:size])
        remaining = remaining[size + 2 :]


def parse_sse(document: str) -> List[Dict[str, str]]:
    events: List[Dict[str, str]] = []
    fields: Dict[str, str] = {}
    data_lines: List[str] = []
    for line in document.replace("\r\n", "\n").split("\n"):
        if not line:
            if data_lines:
                fields["data"] = "\n".join(data_lines)
            if fields:
                events.append(fields)
            fields = {}
            data_lines = []
            continue
        if line.startswith(":"):
            fields["comment"] = line[1:].lstrip()
            continue
        name, separator, value = line.partition(":")
        if not separator:
            continue
        value = value.lstrip(" ")
        if name == "data":
            data_lines.append(value)
        elif name in {"event", "id", "retry"}:
            fields[name] = value
    if data_lines:
        fields["data"] = "\n".join(data_lines)
    if fields:
        events.append(fields)
    return events


def require_strict_log_order(records: Sequence[Dict[str, Any]]) -> None:
    previous = 0
    for record in records:
        sequence = record.get("sequence")
        if not isinstance(sequence, int) or isinstance(sequence, bool):
            raise GateError("workload log record omitted an integer sequence")
        if sequence <= previous:
            raise GateError("workload log records are not strictly ordered")
        previous = sequence


class Evidence:
    def __init__(self, root: pathlib.Path) -> None:
        self.root = root.resolve()
        self.root.mkdir(parents=True, exist_ok=True)

    def write_json(self, name: str, value: Any) -> None:
        self._path(name).write_text(
            json.dumps(value, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    def write_text(self, name: str, value: str) -> None:
        self._path(name).write_text(value, encoding="utf-8")

    def _path(self, name: str) -> pathlib.Path:
        candidate = pathlib.Path(name)
        if candidate.name != name or name in {"", ".", ".."}:
            raise GateError("evidence filename is invalid")
        return self.root / name


class ApiClient:
    def __init__(self, origin: str, token: str, evidence: Evidence) -> None:
        parsed = urllib.parse.urlparse(origin)
        if (
            parsed.scheme != "http"
            or parsed.hostname not in {"127.0.0.1", "localhost"}
            or parsed.username is not None
            or parsed.password is not None
            or parsed.query
            or parsed.fragment
        ):
            raise ValueError("release API origin must be loopback HTTP")
        self.base = origin.rstrip("/") + "/api/v1"
        self.token = token
        self.evidence = evidence
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(
        self,
        method: str,
        path: str,
        expected_statuses: Iterable[int],
        evidence_name: str,
        body: Optional[Dict[str, Any]] = None,
        idempotency_key: Optional[str] = None,
        authenticated: bool = True,
        headers: Optional[Dict[str, str]] = None,
    ) -> Any:
        encoded = None if body is None else json.dumps(body).encode("utf-8")
        request_headers = {"accept": "application/json"}
        if encoded is not None:
            request_headers["content-type"] = "application/json"
        if authenticated:
            request_headers["authorization"] = f"Bearer {self.token}"
        if idempotency_key is not None:
            request_headers["idempotency-key"] = idempotency_key
        if headers:
            request_headers.update(headers)
        request = urllib.request.Request(
            self.base + path,
            data=encoded,
            headers=request_headers,
            method=method,
        )
        try:
            with self.opener.open(request, timeout=10) as response:
                status = response.status
                payload = response.read()
        except urllib.error.HTTPError as error:
            status = error.code
            payload = error.read()
        except (OSError, urllib.error.URLError) as error:
            raise GateError(f"API request {method} {path} failed: {error}") from error
        try:
            document = json.loads(payload)
        except json.JSONDecodeError as error:
            raise GateError(
                f"API request {method} {path} returned invalid JSON"
            ) from error
        self.evidence.write_json(evidence_name, document)
        if status not in set(expected_statuses):
            message = document.get("message") if isinstance(document, dict) else None
            raise GateError(
                f"API request {method} {path} returned HTTP {status}: {message!r}"
            )
        if not isinstance(document, dict) or document.get("code") != status:
            raise GateError(f"API request {method} {path} violated the response wrapper")
        if "data" not in document:
            raise GateError(f"API request {method} {path} omitted response data")
        return document["data"]


def wait_for(
    description: str,
    timeout_seconds: float,
    probe: Callable[[], Optional[Any]],
) -> Any:
    deadline = time.monotonic() + timeout_seconds
    last_error: Optional[Exception] = None
    while time.monotonic() < deadline:
        try:
            result = probe()
            if result is not None:
                return result
        except FatalGateError:
            raise
        except GateError as error:
            last_error = error
        time.sleep(0.25)
    suffix = "" if last_error is None else f": {last_error}"
    raise GateError(f"timed out waiting for {description}{suffix}")


def https_get(
    address: Tuple[str, int],
    hostname: str,
    ca_file: pathlib.Path,
) -> Tuple[int, Dict[str, str], bytes, Dict[str, Any]]:
    context = ssl.create_default_context(cafile=str(ca_file))
    raw = socket.create_connection(address, timeout=5)
    with raw:
        with context.wrap_socket(raw, server_hostname=hostname) as connection:
            connection.settimeout(5)
            certificate = connection.getpeercert()
            connection.sendall(
                (
                    "GET / HTTP/1.1\r\n"
                    f"Host: {hostname}\r\n"
                    "Accept: text/plain\r\n"
                    "Connection: close\r\n\r\n"
                ).encode("ascii")
            )
            chunks = []
            while True:
                chunk = connection.recv(64 * 1024)
                if not chunk:
                    break
                chunks.append(chunk)
    status, headers, body = parse_http_response(b"".join(chunks))
    return status, headers, body, certificate


def first_sse_event(
    origin: str,
    path: str,
    token: str,
    last_event_id: Optional[str] = None,
) -> Tuple[str, Dict[str, str]]:
    parsed = urllib.parse.urlparse(origin)
    if parsed.scheme != "http" or parsed.hostname is None:
        raise ValueError("SSE origin must be HTTP")
    connection = http.client.HTTPConnection(
        parsed.hostname,
        parsed.port or 80,
        timeout=6,
    )
    headers = {
        "accept": "text/event-stream",
        "authorization": f"Bearer {token}",
        "connection": "close",
    }
    if last_event_id is not None:
        headers["last-event-id"] = last_event_id
    connection.request("GET", "/api/v1" + path, headers=headers)
    response = connection.getresponse()
    if response.status != 200:
        body = response.read(4096).decode("utf-8", "replace")
        connection.close()
        raise GateError(f"SSE request returned HTTP {response.status}: {body!r}")
    lines: List[str] = []
    try:
        while True:
            encoded = response.readline()
            if not encoded:
                break
            line = encoded.decode("utf-8", "strict")
            lines.append(line)
            if line in {"\n", "\r\n"} and len(lines) > 1:
                break
    except (OSError, socket.timeout) as error:
        raise GateError("SSE endpoint did not emit a complete event") from error
    finally:
        connection.close()
    document = "".join(lines)
    events = parse_sse(document)
    if len(events) != 1:
        raise GateError("SSE endpoint did not emit exactly one initial event")
    return document, events[0]
