#!/usr/bin/env python3
"""Reject incompatible changes to the committed A3S Cloud OpenAPI contract."""

from __future__ import annotations

import json
import sys
from datetime import date
from pathlib import Path
from typing import Any

HTTP_METHODS = {"delete", "get", "head", "options", "patch", "post", "put", "trace"}
DEPRECATION_FIELDS = (
    "x-a3s-deprecated-since",
    "x-a3s-deprecated-on",
    "x-a3s-sunset-not-before",
    "x-a3s-replacement-operation",
)
MINIMUM_DEPRECATION_DAYS = 180


def load_document(path: str | Path) -> dict[str, Any]:
    with Path(path).open(encoding="utf-8") as stream:
        document = json.load(stream)
    if not isinstance(document, dict):
        raise ValueError(f"OpenAPI document {path} is not an object")
    return document


def parse_version(value: Any, location: str, violations: list[str]) -> tuple[int, int, int] | None:
    if not isinstance(value, str):
        violations.append(f"{location} must be a semantic version")
        return None
    parts = value.split(".")
    if len(parts) != 3 or any(not part.isdigit() for part in parts):
        violations.append(f"{location} must be a semantic version")
        return None
    return tuple(int(part) for part in parts)  # type: ignore[return-value]


def operations(document: dict[str, Any]) -> dict[tuple[str, str], dict[str, Any]]:
    result: dict[tuple[str, str], dict[str, Any]] = {}
    paths = document.get("paths", {})
    if not isinstance(paths, dict):
        return result
    for path, path_item in paths.items():
        if not isinstance(path, str) or not isinstance(path_item, dict):
            continue
        for method, operation in path_item.items():
            if method in HTTP_METHODS and isinstance(operation, dict):
                result[(path, method)] = operation
    return result


def validate_document(document: dict[str, Any], today: date | None = None) -> list[str]:
    violations: list[str] = []
    today = today or date.today()
    if document.get("openapi") != "3.0.3":
        violations.append("openapi must remain 3.0.3 for the v1 contract")
    info = document.get("info")
    if not isinstance(info, dict):
        violations.append("info must be an object")
        return violations
    version = parse_version(info.get("version"), "info.version", violations)
    extension_version = parse_version(
        document.get("x-a3s-api-contract-version"),
        "x-a3s-api-contract-version",
        violations,
    )
    if version is not None and extension_version is not None and version != extension_version:
        violations.append("info.version and x-a3s-api-contract-version must match")
    major = document.get("x-a3s-api-major-version")
    if version is not None and major != version[0]:
        violations.append("x-a3s-api-major-version must match the contract major version")
    minimum_days = document.get("x-a3s-minimum-deprecation-days")
    if not isinstance(minimum_days, int) or minimum_days < MINIMUM_DEPRECATION_DAYS:
        violations.append(
            f"x-a3s-minimum-deprecation-days must be at least {MINIMUM_DEPRECATION_DAYS}"
        )

    contract_operations = operations(document)
    if not contract_operations:
        violations.append("paths must contain at least one operation")
        return violations
    operation_ids: dict[str, tuple[str, str]] = {}
    for (path, method), operation in contract_operations.items():
        location = f"{method.upper()} {path}"
        operation_id = operation.get("operationId")
        if not isinstance(operation_id, str) or not operation_id:
            violations.append(f"{location} must have an operationId")
        elif operation_id in operation_ids:
            previous_path, previous_method = operation_ids[operation_id]
            violations.append(
                f"{location} duplicates operationId {operation_id} from "
                f"{previous_method.upper()} {previous_path}"
            )
        else:
            operation_ids[operation_id] = (path, method)
        if not isinstance(operation.get("tags"), list) or not operation["tags"]:
            violations.append(f"{location} must have at least one tag")
        if not isinstance(operation.get("security"), list):
            violations.append(f"{location} must declare security explicitly")
        if not isinstance(operation.get("responses"), dict) or not operation["responses"]:
            violations.append(f"{location} must declare responses")
        _validate_deprecation(operation, location, version, operation_ids, today, violations)

    for (path, method), operation in contract_operations.items():
        if not operation.get("deprecated"):
            continue
        replacement = operation.get("x-a3s-replacement-operation")
        if replacement == operation.get("operationId"):
            violations.append(f"{method.upper()} {path} cannot replace itself")
        elif replacement not in operation_ids:
            violations.append(
                f"{method.upper()} {path} replacement operation {replacement!r} does not exist"
            )
    return violations


def _validate_deprecation(
    operation: dict[str, Any],
    location: str,
    contract_version: tuple[int, int, int] | None,
    operation_ids: dict[str, tuple[str, str]],
    today: date,
    violations: list[str],
) -> None:
    deprecated = operation.get("deprecated") is True
    present_fields = [field for field in DEPRECATION_FIELDS if field in operation]
    if not deprecated:
        if present_fields:
            violations.append(f"{location} has deprecation metadata without deprecated: true")
        return
    missing_fields = [field for field in DEPRECATION_FIELDS if field not in operation]
    if missing_fields:
        violations.append(f"{location} is missing {', '.join(missing_fields)}")
        return
    since = parse_version(
        operation["x-a3s-deprecated-since"],
        f"{location} x-a3s-deprecated-since",
        violations,
    )
    if since is not None and contract_version is not None and since > contract_version:
        violations.append(f"{location} deprecation version is newer than the contract")
    deprecated_on = _parse_date(operation["x-a3s-deprecated-on"], location, violations)
    sunset = _parse_date(operation["x-a3s-sunset-not-before"], location, violations)
    if deprecated_on is not None and deprecated_on > today:
        violations.append(f"{location} deprecation date is in the future")
    if deprecated_on is not None and sunset is not None:
        window = (sunset - deprecated_on).days
        if window < MINIMUM_DEPRECATION_DAYS:
            violations.append(
                f"{location} deprecation window is {window} days; "
                f"at least {MINIMUM_DEPRECATION_DAYS} are required"
            )


def _parse_date(value: Any, location: str, violations: list[str]) -> date | None:
    if not isinstance(value, str):
        violations.append(f"{location} deprecation dates must use YYYY-MM-DD")
        return None
    try:
        return date.fromisoformat(value)
    except ValueError:
        violations.append(f"{location} deprecation dates must use YYYY-MM-DD")
        return None


def compare_contracts(
    baseline: dict[str, Any], candidate: dict[str, Any], today: date | None = None
) -> list[str]:
    violations = validate_document(candidate, today)
    baseline_violations: list[str] = []
    baseline_version = parse_version(
        baseline.get("info", {}).get("version") if isinstance(baseline.get("info"), dict) else None,
        "baseline info.version",
        baseline_violations,
    )
    candidate_version = parse_version(
        candidate.get("info", {}).get("version") if isinstance(candidate.get("info"), dict) else None,
        "candidate info.version",
        violations,
    )
    violations.extend(baseline_violations)
    if baseline_version is not None and candidate_version is not None:
        if candidate_version < baseline_version:
            violations.append("candidate contract version must not decrease")
        if candidate_version[0] != baseline_version[0]:
            violations.append("openapi/v1.json cannot change its contract major version")

    baseline_operations = operations(baseline)
    candidate_operations = operations(candidate)
    semantic_change = set(baseline_operations) != set(candidate_operations)
    for key, baseline_operation in baseline_operations.items():
        path, method = key
        location = f"{method.upper()} {path}"
        candidate_operation = candidate_operations.get(key)
        if candidate_operation is None:
            violations.append(f"{location} was removed from the v1 contract")
            continue
        semantic_change |= _compare_operation(
            baseline,
            candidate,
            baseline_operation,
            candidate_operation,
            location,
            violations,
        )

    semantic_change |= _compare_component_schemas(baseline, candidate, violations)
    if (
        semantic_change
        and baseline_version is not None
        and candidate_version is not None
        and candidate_version <= baseline_version
    ):
        violations.append("semantic contract changes require a newer minor or patch version")
    return sorted(set(violations))


def _compare_operation(
    baseline_document: dict[str, Any],
    candidate_document: dict[str, Any],
    baseline: dict[str, Any],
    candidate: dict[str, Any],
    location: str,
    violations: list[str],
) -> bool:
    changed = False
    baseline_parameters = _parameters(baseline)
    candidate_parameters = _parameters(candidate)
    if set(baseline_parameters) != set(candidate_parameters):
        changed = True
    for identity, old_parameter in baseline_parameters.items():
        new_parameter = candidate_parameters.get(identity)
        if new_parameter is None:
            violations.append(f"{location} removed {identity[0]} parameter {identity[1]}")
            continue
        changed |= _compare_schema(
            baseline_document,
            candidate_document,
            old_parameter.get("schema", {}),
            new_parameter.get("schema", {}),
            f"{location} parameter {identity[1]}",
            "input",
            violations,
        )
    for identity, parameter in candidate_parameters.items():
        if identity not in baseline_parameters and parameter.get("required") is True:
            violations.append(f"{location} added required {identity[0]} parameter {identity[1]}")

    changed |= _compare_request_body(
        baseline_document, candidate_document, baseline, candidate, location, violations
    )
    changed |= _compare_responses(
        baseline_document, candidate_document, baseline, candidate, location, violations
    )
    if baseline.get("security") != candidate.get("security"):
        changed = True
        if baseline.get("security") == [] and candidate.get("security") != []:
            violations.append(f"{location} changed from public to authenticated")
    if baseline.get("deprecated") != candidate.get("deprecated"):
        changed = True
    return changed


def _parameters(operation: dict[str, Any]) -> dict[tuple[str, str], dict[str, Any]]:
    result: dict[tuple[str, str], dict[str, Any]] = {}
    values = operation.get("parameters", [])
    if not isinstance(values, list):
        return result
    for parameter in values:
        if isinstance(parameter, dict) and isinstance(parameter.get("in"), str) and isinstance(
            parameter.get("name"), str
        ):
            result[(parameter["in"], parameter["name"].lower())] = parameter
    return result


def _compare_request_body(
    baseline_document: dict[str, Any],
    candidate_document: dict[str, Any],
    baseline: dict[str, Any],
    candidate: dict[str, Any],
    location: str,
    violations: list[str],
) -> bool:
    old_body = _resolve(baseline_document, baseline.get("requestBody"))
    new_body = _resolve(candidate_document, candidate.get("requestBody"))
    if old_body is None and new_body is None:
        return False
    if old_body is not None and new_body is None:
        violations.append(f"{location} removed its request body")
        return True
    if old_body is None and isinstance(new_body, dict):
        if new_body.get("required") is True:
            violations.append(f"{location} added a required request body")
        return True
    assert isinstance(old_body, dict) and isinstance(new_body, dict)
    changed = old_body.get("required") != new_body.get("required")
    if old_body.get("required") is not True and new_body.get("required") is True:
        violations.append(f"{location} made its request body required")
    old_content = old_body.get("content", {})
    new_content = new_body.get("content", {})
    if not isinstance(old_content, dict) or not isinstance(new_content, dict):
        return True
    if set(old_content) != set(new_content):
        changed = True
    for media_type, old_media in old_content.items():
        new_media = new_content.get(media_type)
        if not isinstance(new_media, dict):
            violations.append(f"{location} removed request media type {media_type}")
            continue
        if isinstance(old_media, dict):
            changed |= _compare_schema(
                baseline_document,
                candidate_document,
                old_media.get("schema", {}),
                new_media.get("schema", {}),
                f"{location} request {media_type}",
                "input",
                violations,
            )
    return changed


def _compare_responses(
    baseline_document: dict[str, Any],
    candidate_document: dict[str, Any],
    baseline: dict[str, Any],
    candidate: dict[str, Any],
    location: str,
    violations: list[str],
) -> bool:
    old_responses = baseline.get("responses", {})
    new_responses = candidate.get("responses", {})
    if not isinstance(old_responses, dict) or not isinstance(new_responses, dict):
        return True
    changed = set(old_responses) != set(new_responses)
    for status, old_response_value in old_responses.items():
        if status not in new_responses:
            violations.append(f"{location} removed response status {status}")
            continue
        old_response = _resolve(baseline_document, old_response_value)
        new_response = _resolve(candidate_document, new_responses[status])
        if not isinstance(old_response, dict) or not isinstance(new_response, dict):
            changed |= old_response != new_response
            continue
        old_content = old_response.get("content", {})
        new_content = new_response.get("content", {})
        if not isinstance(old_content, dict) or not isinstance(new_content, dict):
            changed = True
            continue
        if set(old_content) != set(new_content):
            changed = True
        for media_type, old_media in old_content.items():
            new_media = new_content.get(media_type)
            if not isinstance(new_media, dict):
                violations.append(f"{location} response {status} removed media type {media_type}")
                continue
            if isinstance(old_media, dict):
                changed |= _compare_schema(
                    baseline_document,
                    candidate_document,
                    old_media.get("schema", {}),
                    new_media.get("schema", {}),
                    f"{location} response {status} {media_type}",
                    "output",
                    violations,
                )
    return changed


def _compare_component_schemas(
    baseline: dict[str, Any], candidate: dict[str, Any], violations: list[str]
) -> bool:
    old_schemas = _schemas(baseline)
    new_schemas = _schemas(candidate)
    changed = set(old_schemas) != set(new_schemas)
    for name, old_schema in old_schemas.items():
        new_schema = new_schemas.get(name)
        if new_schema is None:
            violations.append(f"component schema {name} was removed")
            continue
        changed |= _compare_schema(
            baseline,
            candidate,
            old_schema,
            new_schema,
            f"component schema {name}",
            "output",
            violations,
        )
    return changed


def _schemas(document: dict[str, Any]) -> dict[str, dict[str, Any]]:
    components = document.get("components", {})
    schemas = components.get("schemas", {}) if isinstance(components, dict) else {}
    if not isinstance(schemas, dict):
        return {}
    return {name: schema for name, schema in schemas.items() if isinstance(schema, dict)}


def _compare_schema(
    baseline_document: dict[str, Any],
    candidate_document: dict[str, Any],
    baseline_value: Any,
    candidate_value: Any,
    location: str,
    direction: str,
    violations: list[str],
) -> bool:
    baseline = _resolve(baseline_document, baseline_value)
    candidate = _resolve(candidate_document, candidate_value)
    if baseline == candidate:
        return False
    if not isinstance(baseline, dict) or not isinstance(candidate, dict):
        violations.append(f"{location} changed schema shape")
        return True
    if baseline.get("type") != candidate.get("type"):
        violations.append(f"{location} changed type from {baseline.get('type')} to {candidate.get('type')}")
    old_enum = baseline.get("enum")
    new_enum = candidate.get("enum")
    if isinstance(old_enum, list) and isinstance(new_enum, list):
        removed = [value for value in old_enum if value not in new_enum]
        if removed:
            violations.append(f"{location} removed enum values {removed}")
    old_properties = baseline.get("properties", {})
    new_properties = candidate.get("properties", {})
    if isinstance(old_properties, dict) and isinstance(new_properties, dict):
        for name, old_property in old_properties.items():
            if name not in new_properties:
                violations.append(f"{location} removed property {name}")
                continue
            _compare_schema(
                baseline_document,
                candidate_document,
                old_property,
                new_properties[name],
                f"{location}.{name}",
                direction,
                violations,
            )
        if direction == "input":
            old_required = set(baseline.get("required", []))
            new_required = set(candidate.get("required", []))
            added_required = sorted(new_required - old_required)
            if added_required:
                violations.append(f"{location} added required properties {added_required}")
        else:
            old_required = set(baseline.get("required", []))
            new_required = set(candidate.get("required", []))
            removed_required = sorted(old_required - new_required)
            if removed_required:
                violations.append(f"{location} made response properties optional {removed_required}")
    if "items" in baseline:
        if "items" not in candidate:
            violations.append(f"{location} removed array item schema")
        else:
            _compare_schema(
                baseline_document,
                candidate_document,
                baseline["items"],
                candidate["items"],
                f"{location}[]",
                direction,
                violations,
            )
    if direction == "input":
        for key, comparator in (("minLength", lambda old, new: new > old), ("maximum", lambda old, new: new < old)):
            old_limit = baseline.get(key)
            new_limit = candidate.get(key)
            if isinstance(old_limit, (int, float)) and isinstance(new_limit, (int, float)) and comparator(old_limit, new_limit):
                violations.append(f"{location} narrowed input constraint {key}")
    return True


def _resolve(document: dict[str, Any], value: Any) -> Any:
    seen: set[str] = set()
    while isinstance(value, dict) and isinstance(value.get("$ref"), str):
        reference = value["$ref"]
        if not reference.startswith("#/") or reference in seen:
            return value
        seen.add(reference)
        current: Any = document
        for encoded_part in reference[2:].split("/"):
            part = encoded_part.replace("~1", "/").replace("~0", "~")
            if not isinstance(current, dict) or part not in current:
                return value
            current = current[part]
        value = current
    return value


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: check_contract.py BASELINE_OPENAPI CANDIDATE_OPENAPI", file=sys.stderr)
        return 2
    try:
        violations = compare_contracts(load_document(argv[1]), load_document(argv[2]))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"OpenAPI compatibility check failed: {error}", file=sys.stderr)
        return 2
    if violations:
        print("OpenAPI v1 compatibility violations:", file=sys.stderr)
        for violation in violations:
            print(f"- {violation}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
