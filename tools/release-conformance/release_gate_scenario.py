#!/usr/bin/env python3
"""The public API, TLS, logs, update, rollback, and stop E0 scenario."""

from __future__ import annotations

from typing import Any, Dict, Set

from release_gate_assertions import (
    accept_workload,
    assert_log_stream,
    docker_running,
    wait_for_deployment,
    wait_for_https,
    wait_for_logs,
    wait_for_node,
    wait_for_operation,
    wait_for_route,
)
from release_gate_support import (
    ADMIN_TOKEN_ENV,
    ApiClient,
    Evidence,
    GateError,
    field,
    https_get,
    load_context,
    required_environment,
    service_template,
    uuid_field,
)


RELEASE_A = "release-a"
RELEASE_B = "release-b"
LOG_A = "A3S_CLOUD_E0_RELEASE_A_LOG"
LOG_B = "A3S_CLOUD_E0_RELEASE_B_LOG"


def exercise(args: Any) -> None:
    args.evidence = Evidence(args.evidence_dir)
    token = required_environment(ADMIN_TOKEN_ENV)
    context = load_context(args.context)
    organization_id = context["organizationId"]
    client = ApiClient(args.api_origin, token, args.evidence)
    node = wait_for_node(client, organization_id, args.timeout)

    try:
        https_get(
            (args.gateway_host, args.gateway_port),
            args.hostname,
            args.gateway_ca,
        )
    except (OSError, GateError) as error:
        args.evidence.write_text("https-before-route.txt", f"unavailable: {error}\n")
    else:
        raise GateError("Gateway exposed managed TLS before route acknowledgement")

    accepted_a = accept_workload(client, context, args, RELEASE_A, LOG_A)
    workload_id = uuid_field(accepted_a, "workloadId")
    revision_a = uuid_field(accepted_a, "revisionId")
    deployment_a = uuid_field(accepted_a, "deploymentId")
    wait_for_deployment(
        client,
        organization_id,
        deployment_a,
        revision_a,
        "release-a",
        args.timeout,
        retirement_required=False,
    )
    docker_a = docker_running(
        args.docker_namespace, 1, "release-a", args.evidence, args.timeout
    )

    claim = client.request(
        "POST",
        (
            f"/organizations/{organization_id}/projects/{context['projectId']}"
            f"/environments/{context['environmentId']}/domain-claims"
        ),
        {201},
        "domain-claim.json",
        body={"pattern": args.hostname},
        idempotency_key="e0-clean-host-domain",
    )
    if not isinstance(claim, dict):
        raise GateError("domain claim response is not an object")
    claim_id = uuid_field(claim, "id")
    challenge = field(claim, "challengeValue", str)
    verified = client.request(
        "POST",
        f"/organizations/{organization_id}/domain-claims/{claim_id}/verify",
        {202},
        "domain-claim-verified.json",
        body={"proof": challenge},
        idempotency_key="e0-clean-host-domain-verify",
    )
    if not isinstance(verified, dict) or verified.get("state") != "verified":
        raise GateError("development domain claim did not verify")

    publication = client.request(
        "POST",
        (
            f"/organizations/{organization_id}/projects/{context['projectId']}"
            f"/environments/{context['environmentId']}/routes"
        ),
        {202},
        "route-publication.json",
        body={
            "workloadRevisionId": revision_a,
            "domainClaimId": claim_id,
            "hostname": args.hostname,
            "pathPrefix": "/",
            "portName": "http",
        },
        idempotency_key="e0-clean-host-route",
    )
    if not isinstance(publication, dict) or not isinstance(publication.get("route"), dict):
        raise GateError("route publication response omitted the route")
    route_id = uuid_field(publication["route"], "id")
    route_a = wait_for_route(
        client, organization_id, route_id, revision_a, "release-a", args.timeout
    )
    https_a = wait_for_https(args, RELEASE_A + "\n", "release-a")
    logs_a = wait_for_logs(
        client,
        organization_id,
        workload_id,
        revision_a,
        LOG_A,
        "release-a",
        args.timeout,
    )
    sse_cursor = assert_log_stream(
        args, token, organization_id, workload_id, revision_a, LOG_A
    )

    accepted_b = client.request(
        "POST",
        f"/organizations/{organization_id}/workloads/{workload_id}/deployments",
        {202},
        "workload-release-b-accepted.json",
        body={
            "template": service_template(
                args.artifact_uri,
                args.artifact_digest,
                RELEASE_B,
                LOG_B,
            )
        },
        idempotency_key="e0-clean-host-workload-b",
    )
    if not isinstance(accepted_b, dict):
        raise GateError("update acceptance response is not an object")
    revision_b = uuid_field(accepted_b, "revisionId")
    deployment_b = uuid_field(accepted_b, "deploymentId")
    wait_for_deployment(
        client,
        organization_id,
        deployment_b,
        revision_b,
        "release-b",
        args.timeout,
        retirement_required=True,
    )
    route_b = wait_for_route(
        client, organization_id, route_id, revision_b, "release-b", args.timeout
    )
    https_b = wait_for_https(args, RELEASE_B + "\n", "release-b")
    wait_for_logs(
        client,
        organization_id,
        workload_id,
        revision_b,
        LOG_B,
        "release-b",
        args.timeout,
    )
    docker_b = docker_running(
        args.docker_namespace, 1, "release-b", args.evidence, args.timeout
    )

    rollback = client.request(
        "POST",
        f"/organizations/{organization_id}/workloads/{workload_id}/rollback",
        {202},
        "workload-rollback-accepted.json",
        body={"revisionId": revision_a},
        idempotency_key="e0-clean-host-rollback",
    )
    if not isinstance(rollback, dict):
        raise GateError("rollback acceptance response is not an object")
    if rollback.get("rollbackSourceRevisionId") != revision_a:
        raise GateError("rollback response omitted exact source lineage")
    rollback_revision = uuid_field(rollback, "revisionId")
    if rollback_revision in {revision_a, revision_b}:
        raise GateError("rollback reused an existing revision identity")
    rollback_deployment = uuid_field(rollback, "deploymentId")
    rollback_operation = uuid_field(rollback, "operationId")
    wait_for_deployment(
        client,
        organization_id,
        rollback_deployment,
        rollback_revision,
        "rollback",
        args.timeout,
        retirement_required=True,
    )
    route_rollback = wait_for_route(
        client,
        organization_id,
        route_id,
        rollback_revision,
        "rollback",
        args.timeout,
    )
    https_rollback = wait_for_https(args, RELEASE_A + "\n", "rollback")
    wait_for_logs(
        client,
        organization_id,
        workload_id,
        rollback_revision,
        LOG_A,
        "rollback",
        args.timeout,
    )
    retained_a = wait_for_logs(
        client,
        organization_id,
        workload_id,
        revision_a,
        LOG_A,
        "release-a-retained",
        args.timeout,
    )
    docker_rollback = docker_running(
        args.docker_namespace, 1, "rollback", args.evidence, args.timeout
    )
    operation = wait_for_operation(
        client,
        organization_id,
        rollback_operation,
        "rollback",
        args.timeout,
    )
    if operation.get("rollbackSourceRevisionId") != revision_a:
        raise GateError("rollback operation omitted exact source lineage")

    gateway_revisions = [
        field(route_a, "gatewayRevision", int),
        field(route_b, "gatewayRevision", int),
        field(route_rollback, "gatewayRevision", int),
    ]
    if gateway_revisions != sorted(set(gateway_revisions)):
        raise GateError("Gateway revisions did not advance exactly across cutovers")
    active_container_ids: Set[str] = {
        docker_a[0].split("\t", 1)[0],
        docker_b[0].split("\t", 1)[0],
        docker_rollback[0].split("\t", 1)[0],
    }
    if len(active_container_ids) != 3:
        raise GateError("update or rollback reused a prior Docker resource")

    stopped = client.request(
        "POST",
        f"/organizations/{organization_id}/workloads/{workload_id}/stop",
        {202},
        "workload-stop-accepted.json",
        body={},
        idempotency_key="e0-clean-host-stop",
    )
    if not isinstance(stopped, dict):
        raise GateError("workload stop response is not an object")
    stop_operation = uuid_field(stopped, "operationId")
    wait_for_operation(
        client,
        organization_id,
        stop_operation,
        "stop",
        args.timeout,
    )
    docker_running(args.docker_namespace, 0, "stopped", args.evidence, args.timeout)
    final_workload = client.request(
        "GET",
        f"/organizations/{organization_id}/workloads/{workload_id}",
        {200},
        "workload-final.json",
    )
    if (
        not isinstance(final_workload, dict)
        or final_workload.get("desiredState") != "stopped"
        or final_workload.get("activeRevision") is not None
    ):
        raise GateError("workload stop did not clear the active revision")

    args.evidence.write_json(
        "scenario-summary.json",
        {
            "organizationId": organization_id,
            "projectId": context["projectId"],
            "environmentId": context["environmentId"],
            "nodeId": node["id"],
            "workloadId": workload_id,
            "routeId": route_id,
            "sourceRevisionId": revision_a,
            "updatedRevisionId": revision_b,
            "rollbackRevisionId": rollback_revision,
            "gatewayRevisions": gateway_revisions,
            "activeContainerIds": sorted(active_container_ids),
            "sseCursor": sse_cursor,
            "httpsBodies": [
                https_a["body"],
                https_b["body"],
                https_rollback["body"],
            ],
            "sourceLogRecords": len(logs_a["records"]),
            "retainedSourceLogRecords": len(retained_a["records"]),
            "stopped": True,
        },
    )
