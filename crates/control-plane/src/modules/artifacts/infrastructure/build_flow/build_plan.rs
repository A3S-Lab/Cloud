use super::BuildFlowConfig;
use crate::modules::artifacts::domain::{BuildRun, BuildRunStatus, BuildSource};
use crate::modules::sources::domain::BuildPlatform;
use a3s_box_runtime::{BoxBuildPlan, BuildCachePolicy};
use a3s_cloud_contracts::{
    validate_cloud_artifact, NodeBoxBuildCacheInput, NodeBoxBuildOutput, NodeBoxBuildPlan,
    NodeBoxBuildRequest,
};
use a3s_runtime::contract::ArtifactRef;
use std::collections::BTreeMap;

pub(super) fn project_build_request(
    config: &BuildFlowConfig,
    build: &BuildRun,
    source: &BuildSource,
    parent_output: Option<&NodeBoxBuildOutput>,
) -> Result<NodeBoxBuildRequest, String> {
    validate_projection(build, source, parent_output)?;
    let input = build
        .input_artifact
        .as_ref()
        .ok_or_else(|| "Box build projection requires a prepared input Artifact".to_owned())?;
    let source_artifact = ArtifactRef {
        uri: input.uri.clone(),
        digest: input.digest.clone(),
        media_type: input.media_type.clone(),
    };
    validate_cloud_artifact(&source_artifact)?;

    let parent_caches = parent_output
        .map(parent_caches_by_platform)
        .transpose()?
        .unwrap_or_default();
    let mut plans = Vec::with_capacity(source.recipe.platforms().len());
    for platform in source.recipe.platforms() {
        let plan = canonical_plan(&source.recipe, platform)?;
        let plan_digest = plan.canonical_digest().map_err(|error| error.to_string())?;
        let cache = parent_caches
            .get(platform.as_str())
            .map(|cache| {
                if cache.receipt.source_digest != source_artifact.digest
                    || cache.receipt.plan_digest != plan_digest
                {
                    return Err(format!(
                        "parent Box cache for {} changed its source or plan identity",
                        platform.as_str()
                    ));
                }
                Ok(NodeBoxBuildCacheInput {
                    artifact: cache.artifact.artifact.clone(),
                    receipt: cache.receipt.clone(),
                })
            })
            .transpose()?;
        plans.push(NodeBoxBuildPlan {
            operation_id: operation_id(build, platform),
            plan_acl: plan.canonical_acl().map_err(|error| error.to_string())?,
            cache,
        });
    }
    let request = NodeBoxBuildRequest {
        schema: NodeBoxBuildRequest::SCHEMA.into(),
        generation: u64::from(build.attempt),
        source: source_artifact,
        plans,
        assembly_reference: (source.recipe.platforms().len() > 1)
            .then(|| format!("cloud-build-{}-assembly", build.id)),
        output_max_bytes: config.output_max_bytes,
        cache_max_bytes: config.cache_max_bytes,
    };
    request.validate()?;
    Ok(request)
}

fn validate_projection(
    build: &BuildRun,
    source: &BuildSource,
    parent_output: Option<&NodeBoxBuildOutput>,
) -> Result<(), String> {
    source.validate()?;
    if build.organization_id != source.organization_id
        || build.subject != source.subject
        || !matches!(
            build.status,
            BuildRunStatus::Prepared
                | BuildRunStatus::Scheduled
                | BuildRunStatus::Running
                | BuildRunStatus::Validating
                | BuildRunStatus::Publishing
                | BuildRunStatus::Attesting
                | BuildRunStatus::Cancelling
                | BuildRunStatus::CleanupPending
                | BuildRunStatus::Succeeded
                | BuildRunStatus::Failed
                | BuildRunStatus::Cancelled
        )
        || parent_output.is_some() && build.retry_of_build_run_id.is_none()
    {
        return Err("Box build projection does not match durable build identity".into());
    }
    if let Some(output) = parent_output {
        output.validate()?;
    }
    Ok(())
}

fn canonical_plan(
    recipe: &crate::modules::sources::domain::BuildRecipe,
    platform: &BuildPlatform,
) -> Result<BoxBuildPlan, String> {
    let target = recipe
        .target()
        .map(|target| format!("  target = \"{target}\"\n"))
        .unwrap_or_default();
    let source = format!(
        concat!(
            "build \"oci\" {{\n",
            "  cache = \"content-addressed\"\n",
            "  context = \"{}\"\n",
            "  file = \"{}\"\n",
            "  network = \"none\"\n",
            "  platform = \"{}\"\n",
            "  schema = \"a3s.box.build-plan.v1\"\n",
            "{}",
            "}}\n",
        ),
        recipe.context_path(),
        recipe.dockerfile_path(),
        platform.as_str(),
        target,
    );
    let plan = BoxBuildPlan::parse_acl(&source).map_err(|error| error.to_string())?;
    if plan.cache() != BuildCachePolicy::ContentAddressed {
        return Err("Box build projection must use the sole native cache policy".into());
    }
    Ok(plan)
}

fn parent_caches_by_platform(
    output: &NodeBoxBuildOutput,
) -> Result<BTreeMap<String, &a3s_cloud_contracts::NodeBoxBuildCacheOutput>, String> {
    output.validate()?;
    let mut caches = BTreeMap::new();
    for cache in &output.caches {
        let platform = format!(
            "{}/{}",
            cache.receipt.platform.os, cache.receipt.platform.architecture
        );
        if cache.receipt.platform.variant.is_some() || caches.insert(platform, cache).is_some() {
            return Err("parent Box build caches have ambiguous platforms".into());
        }
    }
    Ok(caches)
}

fn operation_id(build: &BuildRun, platform: &BuildPlatform) -> String {
    format!(
        "cloud-build-{}-{}",
        build.id,
        platform.as_str().replace('/', "-")
    )
}
