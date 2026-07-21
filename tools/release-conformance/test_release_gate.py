#!/usr/bin/env python3

import importlib.util
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("release_gate.py")
RUNNER_PATH = pathlib.Path(__file__).with_name("run_clean_host_gate.sh")
SPEC = importlib.util.spec_from_file_location("release_gate", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class ReleaseGateContractTests(unittest.TestCase):
    def test_generated_cloud_config_contains_the_closed_source_policy(self) -> None:
        runner = RUNNER_PATH.read_text(encoding="utf-8")
        cloud_config = runner.split(
            'cat >"$config_dir/cloud.acl" <<ACL\n', maxsplit=1
        )[1].split("\nACL\n", maxsplit=1)[0]

        self.assertIn(
            """sources {
  github_request_timeout_ms = 10000
  github_webhook_secret_env = "A3S_CLOUD_GITHUB_WEBHOOK_SECRET"
  github_webhook_max_body_bytes = 1048576
  github_app_enabled = false
  github_app_slug = ""
  github_app_client_id = ""
  github_app_client_secret_env = ""
  github_app_private_key_env = ""
  github_app_callback_url = ""
  github_connection_state_ttl_ms = 600000
  github_authority_reconcile_interval_ms = 10000
  github_authority_poll_interval_ms = 300000
  github_authority_retry_initial_ms = 1000
  github_authority_retry_max_ms = 60000
  github_authority_batch_size = 100
  checkout_dir = "$state_dir/source-checkouts"
  checkout_timeout_ms = 120000
  checkout_max_files = 100000
  checkout_max_bytes = 268435456
  allowed_repositories = ["https://github.com/A3S-Lab/Cloud"]
  denied_repositories = []
}""",
            cloud_config,
        )

    def test_generated_cloud_config_contains_the_build_flow_contract(self) -> None:
        runner = RUNNER_PATH.read_text(encoding="utf-8")
        cloud_config = runner.split(
            'cat >"$config_dir/cloud.acl" <<ACL\n', maxsplit=1
        )[1].split("\nACL\n", maxsplit=1)[0]

        self.assertIn(
            """builds {
  reconcile_interval_ms = 250
  builder_uri = "oci://docker.io/moby/buildkit@sha256:0eeb84626c0cd01aecae7848c5ed8f095aec279dd936d0cdb5a64110f42ca65b"
  builder_digest = "sha256:0eeb84626c0cd01aecae7848c5ed8f095aec279dd936d0cdb5a64110f42ca65b"
  builder_media_type = "application/vnd.oci.image.index.v1+json"
  buildkit_socket_volume_id = "a3s-cloud-buildkit-v0-31-2"
  input_staging_dir = "$state_dir/build-input-staging"
  input_max_entries = 100000
  input_max_bytes = 536870912
  output_staging_dir = "$state_dir/build-output-staging"
  output_max_entries = 100000
  output_max_expanded_bytes = 1073741824
  oci_max_blobs = 10000
  oci_max_bytes = 1073741824
  command_ttl_ms = 900000
  runtime_execution_timeout_ms = 600000
  observation_poll_ms = 250
  convergence_timeout_ms = 1800000
  cleanup_timeout_ms = 300000
  cpu_millis = 2000
  memory_bytes = 1073741824
  pids = 512
  output_max_bytes = 536870912
}""",
            cloud_config,
        )

    def test_generated_configs_bound_artifact_storage_and_transfer(self) -> None:
        runner = RUNNER_PATH.read_text(encoding="utf-8")
        cloud_config = runner.split(
            'cat >"$config_dir/cloud.acl" <<ACL\n', maxsplit=1
        )[1].split("\nACL\n", maxsplit=1)[0]
        node_config = runner.split(
            'cat >"$config_dir/node.acl" <<ACL\n', maxsplit=1
        )[1].split("\nACL\n", maxsplit=1)[0]

        self.assertIn(
            """artifacts {
  store_dir = "$state_dir/artifacts"
  max_blob_bytes = 1073741824
  transfer_timeout_ms = 900000
}""",
            cloud_config,
        )
        self.assertIn("artifact_transfer_timeout_ms = 900000", node_config)
        self.assertIn(
            """artifacts {
  max_blob_bytes = 1073741824
  max_entries = 100000
  max_file_bytes = 1073741824
  max_expanded_bytes = 4294967296
}""",
            node_config,
        )

    def test_service_template_binds_the_exact_digest_and_release_marker(self) -> None:
        digest = f"sha256:{'a' * 64}"
        template = MODULE.service_template(
            f"oci://127.0.0.1:50020/a3s/busybox@{digest}",
            digest,
            "release-a",
            "A3S_CLOUD_E0_RELEASE_A_LOG",
        )

        self.assertEqual(template["artifact"]["expectedDigest"], digest)
        self.assertEqual(template["health"]["path"], "/")
        self.assertEqual(template["ports"], [{"name": "http", "containerPort": 8080}])
        command = template["process"]["args"][1]
        self.assertIn("A3S_CLOUD_E0_RELEASE_A_LOG", command)
        self.assertIn("release-a", command)

    def test_http_response_parser_decodes_chunked_gateway_bodies(self) -> None:
        status, headers, body = MODULE.parse_http_response(
            b"HTTP/1.1 200 OK\r\n"
            b"content-type: text/plain\r\n"
            b"transfer-encoding: chunked\r\n\r\n"
            b"9\r\nrelease-a\r\n1\r\n\n\r\n0\r\n\r\n"
        )

        self.assertEqual(status, 200)
        self.assertEqual(headers["content-type"], "text/plain")
        self.assertEqual(body, b"release-a\n")

    def test_sse_parser_preserves_event_identity_and_comments(self) -> None:
        events = MODULE.parse_sse(
            ": keepalive\n\n"
            "id: sha256:abc\n"
            "event: snapshot\n"
            'data: [{"sequence":1,"data":"marker"}]\n\n'
        )

        self.assertEqual(events[0], {"comment": "keepalive"})
        self.assertEqual(events[1]["id"], "sha256:abc")
        self.assertEqual(events[1]["event"], "snapshot")
        self.assertIn('"sequence":1', events[1]["data"])

    def test_log_records_must_be_strictly_ordered(self) -> None:
        MODULE.require_strict_log_order(
            [
                {"sequence": 1, "kind": "data"},
                {"sequence": 2, "kind": "gap"},
                {"sequence": 4, "kind": "data"},
            ]
        )

        with self.assertRaises(MODULE.GateError):
            MODULE.require_strict_log_order(
                [
                    {"sequence": 2, "kind": "data"},
                    {"sequence": 2, "kind": "data"},
                ]
            )

    def test_digest_validation_rejects_tags_and_uppercase_hex(self) -> None:
        self.assertEqual(
            MODULE.validate_digest(f"sha256:{'0' * 64}"),
            f"sha256:{'0' * 64}",
        )
        for invalid in (
            "latest",
            f"sha256:{'A' * 64}",
            f"sha256:{'0' * 63}",
        ):
            with self.subTest(invalid=invalid):
                with self.assertRaises(ValueError):
                    MODULE.validate_digest(invalid)


if __name__ == "__main__":
    unittest.main()
