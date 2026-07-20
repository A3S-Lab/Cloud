use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BuildPlatform(String);

impl BuildPlatform {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        match value.as_str() {
            "linux/amd64" | "linux/arm64" => Ok(Self(value)),
            _ => Err("build platform must be linux/amd64 or linux/arm64".into()),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildRecipe {
    schema: String,
    kind: String,
    context_path: String,
    dockerfile_path: String,
    target: Option<String>,
    platforms: Vec<BuildPlatform>,
}

impl BuildRecipe {
    pub const SCHEMA: &'static str = "a3s.cloud.build-recipe.v1";
    pub const DOCKERFILE_KIND: &'static str = "dockerfile";

    pub fn dockerfile(
        schema: &str,
        kind: &str,
        context_path: &str,
        dockerfile_path: &str,
        target: Option<&str>,
        platforms: Vec<String>,
    ) -> Result<Self, String> {
        if schema != Self::SCHEMA {
            return Err(format!("build recipe schema must be {}", Self::SCHEMA));
        }
        if kind != Self::DOCKERFILE_KIND {
            return Err("build recipe kind is not supported".into());
        }
        let context_path = normalize_repository_path(context_path, true)?;
        let dockerfile_path = normalize_repository_path(dockerfile_path, false)?;
        let target = target.map(parse_target).transpose()?;
        if platforms.is_empty() || platforms.len() > 8 {
            return Err("build recipe must contain between 1 and 8 platforms".into());
        }
        let platform_count = platforms.len();
        let platforms = platforms
            .into_iter()
            .map(BuildPlatform::parse)
            .collect::<Result<BTreeSet<_>, _>>()?;
        if platforms.len() != platform_count {
            return Err("build recipe platforms must be unique".into());
        }
        Ok(Self {
            schema: Self::SCHEMA.into(),
            kind: Self::DOCKERFILE_KIND.into(),
            context_path,
            dockerfile_path,
            target,
            platforms: platforms.into_iter().collect(),
        })
    }

    pub fn validate(self) -> Result<Self, String> {
        Self::dockerfile(
            &self.schema,
            &self.kind,
            &self.context_path,
            &self.dockerfile_path,
            self.target.as_deref(),
            self.platforms
                .into_iter()
                .map(|platform| platform.0)
                .collect(),
        )
    }

    pub fn digest(&self) -> Result<String, String> {
        let canonical = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode canonical build recipe: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn context_path(&self) -> &str {
        &self.context_path
    }

    pub fn dockerfile_path(&self) -> &str {
        &self.dockerfile_path
    }

    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn platforms(&self) -> &[BuildPlatform] {
        &self.platforms
    }
}

fn normalize_repository_path(value: &str, allow_root: bool) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('/')
        || value.contains(['\0', '\\', '%'])
    {
        return Err("build recipe path must be a bounded relative POSIX path".into());
    }
    let value = value.strip_prefix("./").unwrap_or(value);
    if value == "." {
        return allow_root
            .then(|| ".".to_owned())
            .ok_or_else(|| "build recipe file path cannot be the repository root".into());
    }
    let segments = value.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| {
        segment.is_empty()
            || matches!(*segment, "." | "..")
            || !segment.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@' | b'+')
            })
    }) {
        return Err("build recipe path contains an unsafe segment".into());
    }
    Ok(segments.join("/"))
}

fn parse_target(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("Dockerfile target is invalid".into());
    }
    Ok(value.to_owned())
}
