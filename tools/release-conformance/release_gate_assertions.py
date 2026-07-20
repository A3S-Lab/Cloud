#!/usr/bin/env python3
"""Polling and boundary assertions for the clean-host E0 scenario."""

from __future__ import annotations

import argparse
import json
import subprocess
from typing import Any, Dict, List, Optional

from release_gate_support import (
    ApiClient,
    Evidence,
    FatalGateError,
    GateError,
    field,
    first_sse_event,
    https_get,
    require_strict_log_order,
    service_template,
    uuid_field,
    wait_for,
)


def wait_for_node(
    client: ApiClient,
    organization_id: str,
    timeout: float,
) -> Dict[str, Any]:
    def probe() -> Optional[Dict[str, Any]]:
        nodes = client.request(
            "GET",
            f"/organizations/{organization_id}/nodes",
            {200},
            "node-ready.json",
        )
        if not isinstance(nodes, list):
            raise FatalGateError("node list response is not an array")
        if len(nodes) > 1:
            raise FatalGateError("clean-host gate enrolled more than one node")
        if not nodes:
            return None
        node = nodes[0]
        if not isinstance(node, dict):
            raise FatalGateError("node response is not an object")
        if node.get("state") != "ready" or node.get("availability") != "online":
            return None
        if node.get("runtimeProviderId") != "docker":
            raise FatalGateError("release node did not advertise the Docker provider")
        uuid_field(node, "id")
        return node

    return wait_for("one ready Docker node", timeout, probe)


def wait_for_deployment(
    client: ApiClient,
    organization_id: str,
    deployment_id: str,
    revision_id: str,
    label: str,
    timeout: float,
    retirement_required: bool,
) -> Dict[str, Any]:
    def probe() -> Optional[Dict[str, Any]]:
        deployment = client.request(
            "GET",
            f"/organizations/{organization_id}/deployments/{deployment_id}",
            {200},
            f"deployment-{label}.json",
        )
        if not isinstance(deployment, dict):
            raise FatalGateError("deployment response is not an object")
        status = deployment.get("status")
        if status in {"failed", "cancelled"}:
            raise FatalGateError(
                f"deployment {label} became {status}: {deployment.get('failure')!r}"
            )
        if status != "active":
            return None
        revision = deployment.get("revision")
        observation = deployment.get("observedRuntime")
        operation = deployment.get("operation")
        if not isinstance(revision, dict) or revision.get("id") != revision_id:
            raise FatalGateError(f"deployment {label} selected the wrong revision")
        if (
            not isinstance(observation, dict)
            or observation.get("state") != "running"
            or observation.get("healthState") != "healthy"
        ):
            return None
        if not isinstance(operation, dict) or operation.get("status") != "succeeded":
            return None
        if retirement_required and not isinstance(
            deployment.get("retirementCommandId"), str
        ):
            raise FatalGateError(f"deployment {label} omitted its retirement command")
        return deployment

    return wait_for(f"{label} deployment activation", timeout, probe)


def wait_for_route(
    client: ApiClient,
    organization_id: str,
    route_id: str,
    revision_id: str,
    label: str,
    timeout: float,
) -> Dict[str, Any]:
    def probe() -> Optional[Dict[str, Any]]:
        route = client.request(
            "GET",
            f"/organizations/{organization_id}/routes/{route_id}",
            {200},
            f"route-{label}.json",
        )
        if not isinstance(route, dict):
            raise FatalGateError("route response is not an object")
        if route.get("state") == "rejected":
            raise FatalGateError(f"route {label} was rejected: {route.get('failure')!r}")
        if (
            route.get("state") == "active"
            and route.get("workloadRevisionId") == revision_id
        ):
            return route
        return None

    return wait_for(f"{label} route activation", timeout, probe)


def wait_for_logs(
    client: ApiClient,
    organization_id: str,
    workload_id: str,
    revision_id: str,
    marker: str,
    label: str,
    timeout: float,
) -> Dict[str, Any]:
    path = (
        f"/organizations/{organization_id}/workloads/{workload_id}"
        f"/revisions/{revision_id}/logs?limit=100&stream=stdout"
    )

    def probe() -> Optional[Dict[str, Any]]:
        page = client.request("GET", path, {200}, f"logs-{label}.json")
        if not isinstance(page, dict) or not isinstance(page.get("records"), list):
            raise FatalGateError("workload log response omitted records")
        records = page["records"]
        require_strict_log_order(records)
        if any(
            isinstance(record, dict)
            and isinstance(record.get("data"), str)
            and marker in record["data"]
            for record in records
        ):
            return page
        return None

    return wait_for(f"{label} ordered workload logs", timeout, probe)


def assert_log_stream(
    args: argparse.Namespace,
    token: str,
    organization_id: str,
    workload_id: str,
    revision_id: str,
    marker: str,
) -> str:
    path = (
        f"/organizations/{organization_id}/workloads/{workload_id}"
        f"/revisions/{revision_id}/logs/stream?limit=16&stream=stdout"
    )
    raw, event = first_sse_event(args.api_origin, path, token)
    args.evidence.write_text("logs-release-a-sse.txt", raw)
    if event.get("event") != "records" or not event.get("id", "").startswith("v1:"):
        raise GateError("live log stream omitted its resumable records event")
    try:
        page = json.loads(event.get("data", ""))
    except json.JSONDecodeError as error:
        raise GateError("live log stream emitted invalid JSON") from error
    if not isinstance(page, dict) or not isinstance(page.get("records"), list):
        raise GateError("live log stream payload omitted records")
    require_strict_log_order(page["records"])
    if marker not in event.get("data", ""):
        raise GateError("live log stream omitted the release marker")

    resumed_raw, resumed = first_sse_event(
        args.api_origin,
        path,
        token,
        last_event_id=event["id"],
    )
    args.evidence.write_text("logs-release-a-sse-resumed.txt", resumed_raw)
    if resumed.get("comment") != "keepalive" or "data" in resumed:
        raise GateError("live log reconnect replayed acknowledged records")
    return event["id"]


def docker_running(
    namespace: str,
    expected_count: int,
    label: str,
    evidence: Evidence,
    timeout: float,
) -> List[str]:
    def probe() -> Optional[List[str]]:
        result = subprocess.run(
            [
                "docker",
                "ps",
                "--filter",
                f"label=a3s.cloud.namespace={namespace}",
                "--format",
                "{{.ID}}\t{{.Names}}",
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=10,
        )
        if result.returncode != 0:
            raise GateError(f"Docker inventory failed: {result.stderr.strip()}")
        lines = [line for line in result.stdout.splitlines() if line]
        if len(lines) == expected_count:
            evidence.write_text(f"docker-running-{label}.txt", result.stdout)
            return lines
        return None

    return wait_for(f"{expected_count} running Docker units for {label}", timeout, probe)


def wait_for_https(
    args: argparse.Namespace,
    expected_body: str,
    label: str,
) -> Dict[str, Any]:
    def probe() -> Optional[Dict[str, Any]]:
        try:
            status, headers, body, certificate = https_get(
                (args.gateway_host, args.gateway_port),
                args.hostname,
                args.gateway_ca,
            )
        except (OSError, GateError):
            return None
        if status != 200 or body.decode("utf-8", "strict") != expected_body:
            return None
        sans = [
            value
            for kind, value in certificate.get("subjectAltName", ())
            if kind == "DNS"
        ]
        result = {
            "status": status,
            "contentType": headers.get("content-type"),
            "body": body.decode("utf-8", "strict"),
            "dnsNames": sans,
        }
        args.evidence.write_json(f"https-{label}.json", result)
        return result

    return wait_for(f"{label} managed TLS response", args.timeout, probe)


def wait_for_operation(
    client: ApiClient,
    organization_id: str,
    operation_id: str,
    label: str,
    timeout: float,
) -> Dict[str, Any]:
    def probe() -> Optional[Dict[str, Any]]:
        operations = client.request(
            "GET",
            f"/organizations/{organization_id}/operations?limit=100",
            {200},
            f"operations-{label}.json",
        )
        if not isinstance(operations, list):
            raise FatalGateError("operation list is not an array")
        operation = next(
            (
                item
                for item in operations
                if isinstance(item, dict) and item.get("id") == operation_id
            ),
            None,
        )
        if operation is None:
            return None
        if operation.get("status") in {"failed", "cancelled"}:
            raise FatalGateError(
                f"operation {label} became {operation.get('status')}: "
                f"{operation.get('error')!r}"
            )
        return operation if operation.get("status") == "succeeded" else None

    return wait_for(f"{label} operation completion", timeout, probe)


def accept_workload(
    client: ApiClient,
    context: Dict[str, str],
    args: argparse.Namespace,
    release: str,
    marker: str,
) -> Dict[str, Any]:
    accepted = client.request(
        "POST",
        (
            f"/organizations/{context['organizationId']}"
            f"/projects/{context['projectId']}"
            f"/environments/{context['environmentId']}/workloads"
        ),
        {202},
        "workload-release-a-accepted.json",
        body={
            "name": "E0 clean-host service",
            "template": service_template(
                args.artifact_uri,
                args.artifact_digest,
                release,
                marker,
            ),
        },
        idempotency_key="e0-clean-host-workload-a",
    )
    if not isinstance(accepted, dict):
        raise GateError("workload acceptance response is not an object")
    return accepted
