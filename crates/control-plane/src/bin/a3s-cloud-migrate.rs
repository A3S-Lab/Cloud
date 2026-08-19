use a3s_cloud_control_plane::{infrastructure::migrate_postgres, CloudConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/cloud.acl".to_owned());
    let config = CloudConfig::load(path)?;
    let migration_postgres_url = config.migration_postgres_url()?;
    let report = migrate_postgres(&migration_postgres_url, config.postgres.max_connections).await?;

    if report.is_up_to_date() {
        println!("A3S Cloud PostgreSQL schema is already up to date");
    } else {
        println!(
            "A3S Cloud PostgreSQL migrations applied: {}",
            report.applied.join(",")
        );
    }
    Ok(())
}
