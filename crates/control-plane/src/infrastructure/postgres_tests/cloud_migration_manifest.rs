use super::{cloud_migrations, CLOUD_MIGRATION_COUNT, LATEST_CLOUD_MIGRATION_VERSION};

#[test]
fn migration_manifest_summary_matches_the_registered_contiguous_chain() {
    let migrations = cloud_migrations();
    assert_eq!(
        migrations.len(),
        usize::try_from(CLOUD_MIGRATION_COUNT).expect("migration count fits usize")
    );
    assert_eq!(
        migrations.last().map(|migration| migration.version()),
        Some(LATEST_CLOUD_MIGRATION_VERSION)
    );

    for (index, migration) in migrations.iter().enumerate() {
        let expected = index + 1;
        let actual = migration
            .version()
            .parse::<usize>()
            .expect("Cloud migration versions must be numeric");
        assert_eq!(
            actual,
            expected,
            "Cloud migration {} is not the expected contiguous version {expected:03}",
            migration.version()
        );
    }
}
