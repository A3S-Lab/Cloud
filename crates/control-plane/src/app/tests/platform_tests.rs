use super::*;

#[tokio::test]
async fn boot_shell_exposes_wrapped_platform_and_health_responses() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects)?;
    let platform = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/platform")
                .with_header("accept", "application/json")
                .with_header("x-request-id", "018f3f56-8d4a-7c2a-9f13-5ab3d245d701"),
        )
        .await?;
    let body = response_json(&platform)?;
    assert_eq!(platform.status(), 200);
    assert_eq!(body["code"], 200);
    assert_eq!(body["data"]["name"], "a3s-cloud");
    assert_eq!(body["requestId"], "018f3f56-8d4a-7c2a-9f13-5ab3d245d701");

    let health = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/health/live")
                .with_header("accept", "application/json"),
        )
        .await?;
    let body = response_json(&health)?;
    assert_eq!(body["data"]["status"], "up");

    let readiness = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/health/ready")
                .with_header("accept", "application/json"),
        )
        .await?;
    let body = response_json(&readiness)?;
    assert_eq!(body["data"]["status"], "up");
    Ok(())
}
