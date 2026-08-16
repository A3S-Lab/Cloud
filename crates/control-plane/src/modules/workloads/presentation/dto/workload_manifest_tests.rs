use super::*;

const DIRECT: &str = r#"
version = 1

workload "api" {
  placement {
    node_pool_id = "019c0000-0000-7000-8000-000000000010"
  }
  artifact {
    uri = "oci://registry.example.test/api@sha256:abc"
    expected_digest = "sha256:abc"
  }
  process {
    command = ["/app"]
    args = ["serve"]
    working_directory = "/srv"
    environment "RUST_LOG" {
      value = "info"
    }
  }
  resources {
    cpu_millis = 250
    memory_bytes = 67108864
    pids = 64
    ephemeral_storage_bytes = 134217728
  }
  port "http" {
    container_port = 8080
  }
  health {
    port_name = "http"
    path = "/health"
    interval_ms = 1000
    timeout_ms = 500
    healthy_threshold = 1
    unhealthy_threshold = 3
    stabilization_window_ms = 1000
  }
  secret "database" {
    secret_id = "019c0000-0000-7000-8000-000000000001"
    version = 2
    environment {
      variable = "DATABASE_URL"
    }
  }
}
"#;

#[test]
fn parses_closed_direct_workload_manifest() {
    let manifest = parse_workload_manifest(DIRECT.as_bytes()).expect("valid workload ACL");
    assert_eq!(manifest.name, "api");
    assert_eq!(
        manifest.node_pool_id,
        Some(Uuid::parse_str("019c0000-0000-7000-8000-000000000010").expect("node pool ID"))
    );
    assert_eq!(
        manifest.template.artifact.expected_digest.as_deref(),
        Some("sha256:abc")
    );
    assert_eq!(manifest.template.process.environment["RUST_LOG"], "info");
    assert_eq!(manifest.template.ports[0].container_port, 8080);
    assert!(matches!(
        manifest.template.secrets[0].target,
        SecretBindingTargetDto::Environment { ref variable } if variable == "DATABASE_URL"
    ));
}

#[test]
fn parses_source_manifest_only_without_artifact() {
    let source = DIRECT.replace(
        "  artifact {\n    uri = \"oci://registry.example.test/api@sha256:abc\"\n    expected_digest = \"sha256:abc\"\n  }\n",
        "",
    );
    let manifest =
        parse_source_workload_manifest(source.as_bytes()).expect("valid source workload ACL");
    assert_eq!(manifest.name, "api");
    assert!(manifest.node_pool_id.is_some());
    assert_eq!(manifest.template.process.command, ["/app"]);
}

#[test]
fn parses_headless_workload_without_ports_or_health() {
    let headless = DIRECT
        .replace("  port \"http\" {\n    container_port = 8080\n  }\n", "")
        .replace(
            "  health {\n    port_name = \"http\"\n    path = \"/health\"\n    interval_ms = 1000\n    timeout_ms = 500\n    healthy_threshold = 1\n    unhealthy_threshold = 3\n    stabilization_window_ms = 1000\n  }\n",
            "",
        );
    let manifest =
        parse_workload_manifest(headless.as_bytes()).expect("valid headless workload ACL");

    assert!(manifest.template.ports.is_empty());
    assert!(manifest.template.health.is_none());
}

#[test]
fn parses_shipped_examples() {
    let direct = include_str!("../../../../../../../examples/workload.oci.example.acl");
    let source = include_str!("../../../../../../../examples/workload.source.example.acl");
    assert_eq!(
        parse_workload_manifest(direct.as_bytes())
            .expect("valid direct example")
            .name,
        "api"
    );
    assert_eq!(
        parse_source_workload_manifest(source.as_bytes())
            .expect("valid source example")
            .name,
        "api"
    );
}

#[test]
fn rejects_wrong_artifact_shape_and_unknown_fields() {
    let source_without_artifact = DIRECT.replace(
        "  artifact {\n    uri = \"oci://registry.example.test/api@sha256:abc\"\n    expected_digest = \"sha256:abc\"\n  }\n",
        "",
    );
    assert!(parse_workload_manifest(source_without_artifact.as_bytes())
        .expect_err("direct workload without artifact must be rejected")
        .to_string()
        .contains("acl.schema.block_count"));
    assert!(parse_source_workload_manifest(DIRECT.as_bytes())
        .expect_err("source workload with artifact must be rejected")
        .to_string()
        .contains("acl.schema.unknown_block"));
    assert!(
        parse_workload_manifest(DIRECT.replace("cpu_millis", "cpu").as_bytes())
            .expect_err("unknown resource attribute must be rejected")
            .to_string()
            .contains("acl.schema.missing_attribute")
    );
}

#[test]
fn rejects_invalid_versions_targets_numbers_and_utf8() {
    assert!(
        parse_workload_manifest(DIRECT.replace("version = 1", "version = 2").as_bytes())
            .expect_err("unsupported manifest version must be rejected")
            .to_string()
            .contains("version must be 1")
    );
    assert!(parse_workload_manifest(
        DIRECT
            .replace(
                "    environment {\n      variable = \"DATABASE_URL\"\n    }",
                ""
            )
            .as_bytes()
    )
    .expect_err("secret without a target must be rejected")
    .to_string()
    .contains("exactly one target"));
    assert!(
        parse_workload_manifest(DIRECT.replace("pids = 64", "pids = 1.5").as_bytes())
            .expect_err("fractional integer field must be rejected")
            .to_string()
            .contains("non-negative safe integer")
    );
    assert!(parse_workload_manifest(&[0xff])
        .expect_err("invalid UTF-8 manifest must be rejected")
        .to_string()
        .contains("valid UTF-8"));
    assert!(parse_workload_manifest(
        DIRECT
            .replace("019c0000-0000-7000-8000-000000000010", "not-a-node-pool-id")
            .as_bytes()
    )
    .expect_err("invalid node Pool ID must be rejected")
    .to_string()
    .contains("node_pool_id must be a UUID"));
}

#[test]
fn enforces_manifest_resource_limits() {
    let oversized = "#".repeat(WORKLOAD_MANIFEST_MAX_BYTES + 1);
    assert!(parse_workload_manifest(oversized.as_bytes())
        .expect_err("oversized manifest must be rejected")
        .to_string()
        .contains("acl.limit.document_bytes"));
}
