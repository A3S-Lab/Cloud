import copy
import unittest
from datetime import date

from check_contract import compare_contracts, validate_document


def operation(operation_id: str) -> dict:
    return {
        "operationId": operation_id,
        "summary": "List test resources",
        "description": "Returns the documented resource collection used by the compatibility checker tests.",
        "x-a3s-response-data": "A bounded collection of test resources.",
        "tags": ["Tests"],
        "security": [{"bearerAuth": []}],
        "responses": {
            "200": {
                "description": "Success",
                "content": {
                    "application/json": {
                        "schema": {
                            "type": "object",
                            "required": ["id", "name"],
                            "properties": {
                                "id": {"type": "string"},
                                "name": {"type": "string"},
                            },
                        }
                    }
                },
            }
        },
    }


def document(version: str = "1.0.0") -> dict:
    return {
        "openapi": "3.0.3",
        "info": {
            "title": "Test",
            "description": "Documented compatibility-checker fixture.",
            "version": version,
            "contact": {"url": "https://example.com/project"},
            "license": {"name": "MIT"},
        },
        "externalDocs": {"url": "https://example.com/project/api"},
        "tags": [{"name": "Tests", "description": "Compatibility checker test resources."}],
        "components": {
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "Bearer credential used by the documented test API.",
                }
            }
        },
        "paths": {"/resources": {"get": operation("get_resources")}},
        "x-a3s-api-major-version": 1,
        "x-a3s-api-contract-version": version,
        "x-a3s-minimum-deprecation-days": 180,
    }


class ContractCompatibilityTests(unittest.TestCase):
    def test_incomplete_human_documentation_is_rejected(self) -> None:
        candidate = document()
        operation_document = candidate["paths"]["/resources"]["get"]
        del operation_document["description"]
        del candidate["tags"][0]["description"]
        operation_document["requestBody"] = {
            "description": "Binary request used by this documented operation.",
            "content": {
                "application/octet-stream": {
                    "schema": {"type": "string", "format": "binary"}
                }
            },
        }

        violations = validate_document(candidate)
        self.assertIn("GET /resources must have a description", violations)
        self.assertIn("tag 'Tests' must have a description", violations)
        self.assertIn(
            "GET /resources request content application/octet-stream must have an example",
            violations,
        )

    def test_additive_operation_requires_a_version_increment(self) -> None:
        baseline = document()
        candidate = copy.deepcopy(baseline)
        candidate["paths"]["/resources/{resource_id}"] = {"get": operation("get_resource")}

        violations = compare_contracts(baseline, candidate)
        self.assertIn("semantic contract changes require a newer minor or patch version", violations)

        candidate["info"]["version"] = "1.1.0"
        candidate["x-a3s-api-contract-version"] = "1.1.0"
        self.assertEqual(compare_contracts(baseline, candidate), [])

    def test_existing_runtime_validation_can_replace_an_unconstrained_input(self) -> None:
        baseline = document()
        baseline_operation = baseline["paths"]["/resources"]["get"]
        baseline_operation["requestBody"] = {
            "required": True,
            "content": {
                "application/json": {
                    "schema": {"type": "object", "additionalProperties": True}
                }
            },
        }
        candidate = copy.deepcopy(baseline)
        candidate["info"]["version"] = "1.1.0"
        candidate["x-a3s-api-contract-version"] = "1.1.0"
        candidate_body = candidate["paths"]["/resources"]["get"]["requestBody"]
        candidate_body["description"] = "Validated resource creation input."
        candidate_media = candidate_body["content"]["application/json"]
        candidate_media["example"] = {"name": "Example"}
        candidate_media["schema"] = {
            "type": "object",
            "additionalProperties": False,
            "required": ["name"],
            "properties": {"name": {"type": "string", "minLength": 1}},
            "x-a3s-contract-correction": "documents-existing-runtime-validation",
        }

        self.assertEqual(compare_contracts(baseline, candidate), [])

        del candidate_media["schema"]["x-a3s-contract-correction"]
        self.assertTrue(
            any("added required properties" in violation for violation in compare_contracts(baseline, candidate))
        )

    def test_removed_operation_and_response_are_rejected(self) -> None:
        baseline = document()
        candidate = copy.deepcopy(baseline)
        del candidate["paths"]["/resources"]["get"]["responses"]["200"]

        violations = compare_contracts(baseline, candidate)
        self.assertIn("GET /resources removed response status 200", violations)

        candidate = document("1.1.0")
        candidate["paths"] = {}
        violations = compare_contracts(baseline, candidate)
        self.assertIn("GET /resources was removed from the v1 contract", violations)

    def test_new_required_input_is_rejected(self) -> None:
        baseline = document()
        candidate = copy.deepcopy(baseline)
        candidate["info"]["version"] = "1.1.0"
        candidate["x-a3s-api-contract-version"] = "1.1.0"
        candidate["paths"]["/resources"]["get"]["parameters"] = [
            {
                "name": "region",
                "in": "query",
                "required": True,
                "schema": {"type": "string"},
            }
        ]

        self.assertIn(
            "GET /resources added required query parameter region",
            compare_contracts(baseline, candidate),
        )

    def test_removed_response_property_is_rejected(self) -> None:
        baseline = document()
        candidate = copy.deepcopy(baseline)
        candidate["info"]["version"] = "1.1.0"
        candidate["x-a3s-api-contract-version"] = "1.1.0"
        properties = candidate["paths"]["/resources"]["get"]["responses"]["200"]["content"][
            "application/json"
        ]["schema"]["properties"]
        del properties["name"]

        violations = compare_contracts(baseline, candidate)
        self.assertTrue(any("removed property name" in violation for violation in violations))

    def test_deprecation_requires_a_real_replacement_and_six_month_window(self) -> None:
        candidate = document("1.1.0")
        deprecated = candidate["paths"]["/resources"]["get"]
        deprecated.update(
            {
                "deprecated": True,
                "x-a3s-deprecated-since": "1.1.0",
                "x-a3s-deprecated-on": "2026-01-01",
                "x-a3s-sunset-not-before": "2026-02-01",
                "x-a3s-replacement-operation": "get_resource_v2",
            }
        )

        violations = validate_document(candidate, date(2026, 7, 27))
        self.assertTrue(any("deprecation window" in violation for violation in violations))
        self.assertTrue(any("does not exist" in violation for violation in violations))

        candidate["paths"]["/resources-v2"] = {"get": operation("get_resource_v2")}
        deprecated["x-a3s-sunset-not-before"] = "2026-06-30"
        self.assertEqual(validate_document(candidate, date(2026, 7, 27)), [])


if __name__ == "__main__":
    unittest.main()
