use a3s_use_core::{
    PluginHostApplyRequest, PluginHostApplyResult, PluginHostCapabilities,
    PluginHostEnablementPlanRequest, PluginHostEnablementPlanResult, PluginHostManager,
    PluginHostObservationRequest, PluginHostObservationResult, PluginHostPlanRequest,
    PluginHostPlanResult, UseResult,
};

pub(crate) async fn inspect(manager: &dyn PluginHostManager) -> UseResult<PluginHostCapabilities> {
    let capabilities = manager.capabilities().await?;
    capabilities.validate()?;
    Ok(capabilities)
}

pub(crate) async fn plan(
    manager: &dyn PluginHostManager,
    request: &PluginHostPlanRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostPlanResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.plan(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}

pub(crate) async fn apply(
    manager: &dyn PluginHostManager,
    request: &PluginHostApplyRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostApplyResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.apply(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}

pub(crate) async fn plan_enablement(
    manager: &dyn PluginHostManager,
    request: &PluginHostEnablementPlanRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostEnablementPlanResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.plan_enablement(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}

pub(crate) async fn observe(
    manager: &dyn PluginHostManager,
    request: &PluginHostObservationRequest,
) -> UseResult<(PluginHostCapabilities, PluginHostObservationResult)> {
    let capabilities = inspect(manager).await?;
    request.validate_for_capabilities(&capabilities)?;
    let result = manager.observe(request.clone()).await?;
    result.validate_for(request, &capabilities)?;
    Ok((capabilities, result))
}
