#!/usr/bin/env python3
"""Drive the public-API portion of the clean-host E0 release gate."""

from __future__ import annotations

import argparse
import datetime
import json
import pathlib
import sys
from typing import Any, Dict, Optional, Sequence

from release_gate_scenario import exercise
from release_gate_support import (
    ADMIN_TOKEN_ENV,
    BOOTSTRAP_TOKEN_ENV,
    ENROLLMENT_TOKEN_ENV,
    ApiClient,
    Evidence,
    GateError,
    parse_http_response,
    parse_sse,
    require_strict_log_order,
    required_environment,
    service_template,
    uuid_field,
    validate_digest,
    wait_for,
)


def bootstrap(args: argparse.Namespace) -> None:
    evidence = Evidence(args.evidence_dir)
    admin_token = required_environment(ADMIN_TOKEN_ENV)
    bootstrap_token = required_environment(BOOTSTRAP_TOKEN_ENV)
    enrollment_token = required_environment(ENROLLMENT_TOKEN_ENV)
    client = ApiClient(args.api_origin, admin_token, evidence)

    wait_for(
        "control-plane readiness",
        args.timeout,
        lambda: client.request(
            "GET",
            "/health/ready",
            {200},
            "control-plane-ready.json",
            authenticated=False,
        ),
    )
    identity = client.request(
        "POST",
        "/bootstrap",
        {201},
        "bootstrap.json",
        body={
            "organizationName": "E0 Clean Host",
            "tokenName": "release-gate-admin",
            "token": admin_token,
            "expiresAt": None,
        },
        idempotency_key="e0-clean-host-bootstrap",
        authenticated=False,
        headers={"x-a3s-bootstrap-token": bootstrap_token},
    )
    if not isinstance(identity, dict) or not isinstance(identity.get("organization"), dict):
        raise GateError("bootstrap response omitted the organization")
    organization_id = uuid_field(identity["organization"], "id")

    project = client.request(
        "POST",
        f"/organizations/{organization_id}/projects",
        {201},
        "project.json",
        body={"name": "Release Gate"},
        idempotency_key="e0-clean-host-project",
    )
    if not isinstance(project, dict):
        raise GateError("project response is not an object")
    project_id = uuid_field(project, "id")

    environment = client.request(
        "POST",
        f"/organizations/{organization_id}/projects/{project_id}/environments",
        {201},
        "environment.json",
        body={"name": "Acceptance"},
        idempotency_key="e0-clean-host-environment",
    )
    if not isinstance(environment, dict):
        raise GateError("environment response is not an object")
    environment_id = uuid_field(environment, "id")

    expires_at = (
        datetime.datetime.now(datetime.timezone.utc)
        + datetime.timedelta(minutes=20)
    ).isoformat().replace("+00:00", "Z")
    enrollment = client.request(
        "POST",
        f"/organizations/{organization_id}/enrollment-tokens",
        {201},
        "enrollment-token.json",
        body={
            "name": "release-gate-node",
            "token": enrollment_token,
            "expiresAt": expires_at,
        },
        idempotency_key="e0-clean-host-enrollment",
    )
    if not isinstance(enrollment, dict):
        raise GateError("enrollment-token response is not an object")
    uuid_field(enrollment, "id")

    context: Dict[str, Any] = {
        "organizationId": organization_id,
        "projectId": project_id,
        "environmentId": environment_id,
    }
    args.context.parent.mkdir(parents=True, exist_ok=True)
    args.context.write_text(
        json.dumps(context, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    evidence.write_json("release-context.json", context)


def parser() -> argparse.ArgumentParser:
    command = argparse.ArgumentParser(description=__doc__)
    subcommands = command.add_subparsers(dest="command", required=True)

    bootstrap_parser = subcommands.add_parser("bootstrap")
    bootstrap_parser.add_argument("--api-origin", required=True)
    bootstrap_parser.add_argument("--evidence-dir", required=True, type=pathlib.Path)
    bootstrap_parser.add_argument("--context", required=True, type=pathlib.Path)
    bootstrap_parser.add_argument("--timeout", type=float, default=120)
    bootstrap_parser.set_defaults(handler=bootstrap)

    exercise_parser = subcommands.add_parser("exercise")
    exercise_parser.add_argument("--api-origin", required=True)
    exercise_parser.add_argument("--evidence-dir", required=True, type=pathlib.Path)
    exercise_parser.add_argument("--context", required=True, type=pathlib.Path)
    exercise_parser.add_argument("--gateway-host", default="127.0.0.1")
    exercise_parser.add_argument("--gateway-port", required=True, type=int)
    exercise_parser.add_argument("--hostname", required=True)
    exercise_parser.add_argument("--gateway-ca", required=True, type=pathlib.Path)
    exercise_parser.add_argument("--artifact-uri", required=True)
    exercise_parser.add_argument(
        "--artifact-digest", required=True, type=validate_digest
    )
    exercise_parser.add_argument("--docker-namespace", required=True)
    exercise_parser.add_argument("--timeout", type=float, default=180)
    exercise_parser.set_defaults(handler=exercise)
    return command


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parser().parse_args(argv)
    try:
        args.handler(args)
    except (GateError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
