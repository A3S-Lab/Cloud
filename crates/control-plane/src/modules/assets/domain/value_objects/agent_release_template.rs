use a3s_cloud_contracts::{
    agent_harness_compatibility_v1, AgentReleaseManifest, AGENT_RELEASE_LIMITS,
};

pub const AGENT_RELEASE_TEMPLATE_PATH: &str = ".a3s/agent-release.acl";
pub const AGENT_RELEASE_TEMPLATE_MAX_ACL_BYTES: usize = AGENT_RELEASE_LIMITS.max_document_bytes;

/// Canonical A3S Code release template admitted from one pinned Agent commit.
///
/// The Cloud Asset manifest remains `.a3s/asset.acl`; this separate template
/// becomes the final Code-owned `.a3s/asset.acl` only after OCI and provenance
/// digests have been bound by the hosted build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReleaseTemplate {
    canonical_acl: String,
    identity: String,
}

impl AgentReleaseTemplate {
    pub fn parse(source: &str) -> Result<Self, String> {
        let manifest = AgentReleaseManifest::parse(source)
            .map_err(|error| format!("Agent release template is invalid: {error}"))?;
        manifest
            .verify_compatibility(&agent_harness_compatibility_v1())
            .map_err(|error| format!("Agent release template is incompatible: {error}"))?;
        let kinds = manifest
            .provenance()
            .iter()
            .map(|reference| reference.kind())
            .collect::<Vec<_>>();
        if kinds != ["builder", "source"] {
            return Err(
                "Agent release template must declare exactly builder and source provenance".into(),
            );
        }
        Ok(Self {
            canonical_acl: manifest.canonical_acl().into(),
            identity: manifest.identity().into(),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        let admitted = Self::parse(&self.canonical_acl)?;
        if admitted != *self {
            return Err("Agent release template changed its canonical identity".into());
        }
        Ok(())
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    #[cfg(test)]
    pub(crate) fn test_fixture() -> Self {
        Self::parse(test_fixture_acl()).expect("Agent release template fixture")
    }
}

#[cfg(test)]
fn test_fixture_acl() -> &'static str {
    concat!(
        "agent_release {\n",
        "  schema = \"a3s.code.agent-release.v1\"\n",
        "  protocol = \"a3s.code.agent.v1\"\n",
        "  artifact { digest = \"sha256:1111111111111111111111111111111111111111111111111111111111111111\" media_type = \"application/vnd.oci.image.manifest.v1+json\" }\n",
        "  entrypoint { command = \"/usr/bin/a3s\" args = [\"code\", \"harness\", \"--manifest\", \"/app/.a3s/asset.acl\"] }\n",
        "  health { transport = \"http\" port = 8080 readiness_path = \"/health/ready\" liveness_path = \"/health/live\" shutdown_grace_seconds = 30 }\n",
        "  storage { workspace = \"ephemeral\" cache = \"ephemeral\" persistent_data = \"none\" }\n",
        "  capability \"runtime.service\" { level = 1 }\n",
        "  capability \"secrets.external\" { level = 1 }\n",
        "  capability \"workspace.local\" { level = 1 }\n",
        "  provenance \"source\" { uri = \"urn:a3s:source:template\" digest = \"sha256:2222222222222222222222222222222222222222222222222222222222222222\" }\n",
        "  provenance \"builder\" { uri = \"urn:a3s:builder:template\" digest = \"sha256:4444444444444444444444444444444444444444444444444444444444444444\" }\n",
        "}\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_and_canonicalizes_the_code_release_template() {
        let template = AgentReleaseTemplate::test_fixture();
        template.validate().expect("valid template");
        assert!(template.identity().starts_with("sha256:"));
        assert!(template.canonical_acl().ends_with('\n'));
    }

    #[test]
    fn rejects_missing_or_additional_provenance_authority() {
        assert!(AgentReleaseTemplate::parse(&test_fixture_acl().replace(
            "  provenance \"builder\" { uri = \"urn:a3s:builder:template\" digest = \"sha256:4444444444444444444444444444444444444444444444444444444444444444\" }\n",
            "",
        ))
        .is_err());
        let fixture_without_closing_brace = test_fixture_acl()
            .strip_suffix("}\n")
            .expect("fixture closing brace");
        let additional = format!(
            "{fixture_without_closing_brace}  provenance \"supply.chain\" {{ uri = \"urn:a3s:supply-chain:template\" digest = \"sha256:5555555555555555555555555555555555555555555555555555555555555555\" }}\n}}\n"
        );
        assert!(AgentReleaseTemplate::parse(&additional).is_err());
    }
}
