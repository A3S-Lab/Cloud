//! Durable Gateway rate-shaping profile store (Postgres) plus process hydrate.
//!
//! Runtime admission stays process-local via [`InMemoryGatewayRateShapingProfileCatalog`].
//! This store survives process restart and shares authority across api-worker and
//! management without inventing REST catalog CRUD or Delivery rate middleware.

use crate::infrastructure::{PostgresPersistenceError, execute};
use crate::modules::edge::domain::{
    GatewayRateShapingAlgorithm, GatewayRateShapingGcra, GatewayRateShapingProfile,
    GatewayRateShapingTokenBucket,
};
use crate::modules::edge::infrastructure::InMemoryGatewayRateShapingProfileCatalog;
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_orm::expression::Selection;
use a3s_orm::{
    Database, DecodeError, Expression, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row, insert_into, select_from,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

a3s_orm::orm_table! {
    struct GatewayRateShapingProfiles => "gateway_rate_shaping_profiles" {
        profile_id: String => "profile_id",
        policy_revision_digest: String => "policy_revision_digest",
        algorithm_kind: String => "algorithm_kind",
        token_bucket_capacity: Option<i64> => "token_bucket_capacity",
        token_bucket_refill_tokens_per_second: Option<i64> => "token_bucket_refill_tokens_per_second",
        gcra_emission_interval_nanos: Option<i64> => "gcra_emission_interval_nanos",
        gcra_burst_tolerance: Option<i64> => "gcra_burst_tolerance",
        updated_at: DateTime<Utc> => "updated_at",
    }
}

/// Async durable authority for Gateway rate-shaping profile revisions.
#[async_trait]
pub trait IGatewayRateShapingProfileDurableStore: Send + Sync {
    async fn list_all(&self) -> Result<Vec<GatewayRateShapingProfile>, String>;
    async fn upsert(&self, profile: GatewayRateShapingProfile) -> Result<(), String>;
}

/// Process-local durable stand-in for unit tests.
#[derive(Debug, Default)]
pub struct InMemoryGatewayRateShapingProfileDurableStore {
    profiles: RwLock<HashMap<String, GatewayRateShapingProfile>>,
}

impl InMemoryGatewayRateShapingProfileDurableStore {
    pub fn empty() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait]
impl IGatewayRateShapingProfileDurableStore for InMemoryGatewayRateShapingProfileDurableStore {
    async fn list_all(&self) -> Result<Vec<GatewayRateShapingProfile>, String> {
        let guard = self
            .profiles
            .read()
            .map_err(|_| "gateway rate shaping durable store lock poisoned".to_owned())?;
        Ok(guard.values().cloned().collect())
    }

    async fn upsert(&self, profile: GatewayRateShapingProfile) -> Result<(), String> {
        profile.validate()?;
        let mut guard = self
            .profiles
            .write()
            .map_err(|_| "gateway rate shaping durable store lock poisoned".to_owned())?;
        guard.insert(profile.profile_id.clone(), profile);
        Ok(())
    }
}

/// PostgreSQL-backed durable catalog authority.
#[derive(Clone)]
pub struct PostgresGatewayRateShapingProfileDurableStore {
    executor: PostgresExecutor,
}

impl PostgresGatewayRateShapingProfileDurableStore {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IGatewayRateShapingProfileDurableStore for PostgresGatewayRateShapingProfileDurableStore {
    async fn list_all(&self) -> Result<Vec<GatewayRateShapingProfile>, String> {
        let rows = Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                select_from::<GatewayRateShapingProfiles>().select(ProfileSelection),
            )
            .await
            .map_err(|error| error.to_string())?;
        rows.rows
            .into_iter()
            .map(ProfileRow::into_profile)
            .collect::<Result<Vec<_>, _>>()
    }

    async fn upsert(&self, profile: GatewayRateShapingProfile) -> Result<(), String> {
        profile.validate()?;
        let executor = self.executor.clone();
        executor
            .transaction(move |transaction| {
                Box::pin(async move { upsert_profile(transaction, profile).await })
            })
            .await
            .map_err(persist_error)?;
        Ok(())
    }
}

async fn upsert_profile(
    transaction: &PostgresTransaction,
    profile: GatewayRateShapingProfile,
) -> Result<(), PostgresPersistenceError> {
    let now = Utc::now();
    let (kind, capacity, refill, emission, burst) = algorithm_columns(&profile.algorithm);
    execute(
        transaction,
        insert_into::<GatewayRateShapingProfiles>()
            .value(
                GatewayRateShapingProfiles::profile_id(),
                profile.profile_id.clone(),
            )
            .value(
                GatewayRateShapingProfiles::policy_revision_digest(),
                profile.policy_revision_digest.as_str().to_owned(),
            )
            .value(GatewayRateShapingProfiles::algorithm_kind(), kind)
            .value(GatewayRateShapingProfiles::token_bucket_capacity(), capacity)
            .value(
                GatewayRateShapingProfiles::token_bucket_refill_tokens_per_second(),
                refill,
            )
            .value(
                GatewayRateShapingProfiles::gcra_emission_interval_nanos(),
                emission,
            )
            .value(GatewayRateShapingProfiles::gcra_burst_tolerance(), burst)
            .value(GatewayRateShapingProfiles::updated_at(), now)
            .on_conflict(GatewayRateShapingProfiles::profile_id())
            .do_update_from_excluded(GatewayRateShapingProfiles::policy_revision_digest())
            .do_update_from_excluded(GatewayRateShapingProfiles::algorithm_kind())
            .do_update_from_excluded(GatewayRateShapingProfiles::token_bucket_capacity())
            .do_update_from_excluded(
                GatewayRateShapingProfiles::token_bucket_refill_tokens_per_second(),
            )
            .do_update_from_excluded(GatewayRateShapingProfiles::gcra_emission_interval_nanos())
            .do_update_from_excluded(GatewayRateShapingProfiles::gcra_burst_tolerance())
            .do_update_from_excluded(GatewayRateShapingProfiles::updated_at()),
    )
    .await?;
    Ok(())
}

/// Hydrate process-local catalog from durable store, then apply ACL seeds (and persist them).
pub async fn install_gateway_rate_shaping_catalog(
    catalog: &InMemoryGatewayRateShapingProfileCatalog,
    store: &dyn IGatewayRateShapingProfileDurableStore,
    seeds: &[GatewayRateShapingProfile],
) -> Result<(), String> {
    for profile in store.list_all().await? {
        catalog.register(profile)?;
    }
    for profile in seeds {
        catalog.register(profile.clone())?;
        store.upsert(profile.clone()).await?;
    }
    Ok(())
}

fn algorithm_columns(
    algorithm: &GatewayRateShapingAlgorithm,
) -> (String, Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    match algorithm {
        GatewayRateShapingAlgorithm::TokenBucket(bucket) => (
            "token_bucket".into(),
            Some(bucket.capacity as i64),
            Some(bucket.refill_tokens_per_second as i64),
            None,
            None,
        ),
        GatewayRateShapingAlgorithm::Gcra(gcra) => (
            "gcra".into(),
            None,
            None,
            Some(gcra.emission_interval_nanos as i64),
            Some(gcra.burst_tolerance as i64),
        ),
    }
}

fn persist_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

struct ProfileSelection;

impl Selection for ProfileSelection {
    type Output = ProfileRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRateShapingProfiles::profile_id().expression(),
            GatewayRateShapingProfiles::policy_revision_digest().expression(),
            GatewayRateShapingProfiles::algorithm_kind().expression(),
            GatewayRateShapingProfiles::token_bucket_capacity().expression(),
            GatewayRateShapingProfiles::token_bucket_refill_tokens_per_second().expression(),
            GatewayRateShapingProfiles::gcra_emission_interval_nanos().expression(),
            GatewayRateShapingProfiles::gcra_burst_tolerance().expression(),
        ]
    }
}

struct ProfileRow {
    profile_id: String,
    policy_revision_digest: String,
    algorithm_kind: String,
    token_bucket_capacity: Option<i64>,
    token_bucket_refill_tokens_per_second: Option<i64>,
    gcra_emission_interval_nanos: Option<i64>,
    gcra_burst_tolerance: Option<i64>,
}

impl FromRow for ProfileRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            profile_id: decode(row, 0)?,
            policy_revision_digest: decode(row, 1)?,
            algorithm_kind: decode(row, 2)?,
            token_bucket_capacity: decode(row, 3)?,
            token_bucket_refill_tokens_per_second: decode(row, 4)?,
            gcra_emission_interval_nanos: decode(row, 5)?,
            gcra_burst_tolerance: decode(row, 6)?,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

impl ProfileRow {
    fn into_profile(self) -> Result<GatewayRateShapingProfile, String> {
        let digest = Sha256Digest::parse(self.policy_revision_digest.as_str())?;
        let algorithm = match self.algorithm_kind.as_str() {
            "token_bucket" => GatewayRateShapingAlgorithm::TokenBucket(
                GatewayRateShapingTokenBucket {
                    capacity: require_u64(self.token_bucket_capacity, "token_bucket.capacity")?,
                    refill_tokens_per_second: require_u64(
                        self.token_bucket_refill_tokens_per_second,
                        "token_bucket.refill_tokens_per_second",
                    )?,
                },
            ),
            "gcra" => GatewayRateShapingAlgorithm::Gcra(GatewayRateShapingGcra {
                emission_interval_nanos: require_u64(
                    self.gcra_emission_interval_nanos,
                    "gcra.emission_interval_nanos",
                )?,
                burst_tolerance: require_u64(self.gcra_burst_tolerance, "gcra.burst_tolerance")?,
            }),
            other => {
                return Err(format!(
                    "gateway rate shaping durable row has unknown algorithm_kind {other}"
                ));
            }
        };
        GatewayRateShapingProfile::new(self.profile_id, digest, algorithm)
    }
}

fn require_u64(value: Option<i64>, field: &str) -> Result<u64, String> {
    let value = value.ok_or_else(|| format!("gateway rate shaping durable row missing {field}"))?;
    if value <= 0 {
        return Err(format!("gateway rate shaping durable row {field} must be positive"));
    }
    Ok(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{
        GatewayRateShapingAlgorithm, GatewayRateShapingTokenBucket,
    };
    use crate::modules::edge::infrastructure::IGatewayRateShapingProfileCatalog;

    fn digest(byte: u8) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", format!("{:02x}", byte).repeat(32)))
            .expect("digest")
    }

    fn sample(byte: u8) -> GatewayRateShapingProfile {
        GatewayRateShapingProfile::new(
            "public-api-default",
            digest(byte),
            GatewayRateShapingAlgorithm::TokenBucket(GatewayRateShapingTokenBucket {
                capacity: 120,
                refill_tokens_per_second: 60,
            }),
        )
        .expect("profile")
    }

    #[tokio::test]
    async fn install_hydrates_store_then_applies_and_persists_seeds() {
        let store = InMemoryGatewayRateShapingProfileDurableStore::empty();
        store.upsert(sample(0xaa)).await.expect("prior");
        let catalog = InMemoryGatewayRateShapingProfileCatalog::empty();
        let seed = sample(0xbb);
        install_gateway_rate_shaping_catalog(catalog.as_ref(), store.as_ref(), &[seed.clone()])
            .await
            .expect("install");
        let hydrated = catalog
            .find_by_profile_id("public-api-default")
            .expect("catalog");
        assert_eq!(hydrated.policy_revision_digest, digest(0xbb));
        let durable = store.list_all().await.expect("list");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].policy_revision_digest, digest(0xbb));
    }

    #[tokio::test]
    async fn memory_store_upsert_replaces_revision() {
        let store = InMemoryGatewayRateShapingProfileDurableStore::empty();
        store.upsert(sample(0xaa)).await.expect("first");
        store.upsert(sample(0xbb)).await.expect("second");
        let all = store.list_all().await.expect("list");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].policy_revision_digest, digest(0xbb));
    }
}
