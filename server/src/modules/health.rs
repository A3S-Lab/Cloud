use a3s_boot::{BootError, HealthIndicatorResult, HealthModule};

use crate::modules::workflow::infrastructure::PostgresWorkflowRepository;

pub fn module(repository: PostgresWorkflowRepository) -> HealthModule {
    HealthModule::new("health")
        .with_route("/api/health")
        .indicator("postgres", move || {
            let repository = repository.clone();
            async move {
                repository.health_check().await.map_err(|error| {
                    BootError::ServiceUnavailable(format!("PostgreSQL is unavailable: {error}"))
                })?;
                Ok(HealthIndicatorResult::up().with_detail_value("driver", "a3s-orm"))
            }
        })
}
