use a3s_cloud_control_plane::{infrastructure::migrate_postgres, CloudConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/cloud.acl".to_owned());
    let config = CloudConfig::load(path)?;
    let migration_postgres_url = config.migration_postgres_url()?;
    let report = migrate_postgres(
        &migration_postgres_url,
        config.postgres.max_connections,
        &config.postgres.serving_role,
    )
    .await?;

    if report.is_up_to_date() {
        println!("A3S Cloud PostgreSQL schema and serving access are already reconciled");
    } else {
        println!(
            "A3S Cloud PostgreSQL migrations applied: {}; serving access reconciled",
            report.applied.join(",")
        );
    }
    Ok(())
}
