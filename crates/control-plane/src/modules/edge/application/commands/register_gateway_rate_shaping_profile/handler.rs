use super::{RegisterGatewayRateShapingProfile, RegisterGatewayRateShapingProfileResult};
use crate::modules::edge::domain::GatewayRateShapingProfile;
use crate::modules::edge::infrastructure::{
    IGatewayRateShapingProfileCatalog, IGatewayRateShapingProfileDurableStore,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RegisterGatewayRateShapingProfileHandler {
    catalog: Arc<dyn IGatewayRateShapingProfileCatalog>,
    durable_store: Arc<dyn IGatewayRateShapingProfileDurableStore>,
}

impl RegisterGatewayRateShapingProfileHandler {
    pub fn new(
        catalog: Arc<dyn IGatewayRateShapingProfileCatalog>,
        durable_store: Arc<dyn IGatewayRateShapingProfileDurableStore>,
    ) -> Self {
        Self {
            catalog,
            durable_store,
        }
    }
}

impl CommandHandler<RegisterGatewayRateShapingProfile> for RegisterGatewayRateShapingProfileHandler {
    fn execute(
        &self,
        command: RegisterGatewayRateShapingProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<RegisterGatewayRateShapingProfileResult>>,
    > {
        let catalog = Arc::clone(&self.catalog);
        let durable_store = Arc::clone(&self.durable_store);
        Box::pin(async move {
            let profile = match GatewayRateShapingProfile::new(
                command.profile_id,
                command.policy_revision_digest,
                command.algorithm,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = durable_store.upsert(profile.clone()).await {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            if let Err(error) = catalog.register(profile.clone()) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            Ok(Ok(RegisterGatewayRateShapingProfileResult { profile }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{
        GatewayRateShapingAlgorithm, GatewayRateShapingTokenBucket,
    };
    use crate::modules::edge::infrastructure::{
        IGatewayRateShapingProfileCatalog, InMemoryGatewayRateShapingProfileCatalog,
        InMemoryGatewayRateShapingProfileDurableStore,
    };
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use a3s_boot::{CommandHandler, ModuleRef};

    fn digest(byte: u8) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", format!("{:02x}", byte).repeat(32)))
            .expect("digest")
    }

    #[tokio::test]
    async fn registers_profile_into_shared_catalog_and_durable_store() {
        let catalog = InMemoryGatewayRateShapingProfileCatalog::empty();
        let store = InMemoryGatewayRateShapingProfileDurableStore::empty();
        let handler = RegisterGatewayRateShapingProfileHandler::new(
            Arc::clone(&catalog) as Arc<dyn IGatewayRateShapingProfileCatalog>,
            Arc::clone(&store) as Arc<dyn IGatewayRateShapingProfileDurableStore>,
        );
        let result = handler
            .execute(
                RegisterGatewayRateShapingProfile {
                    profile_id: "public-api-default".into(),
                    policy_revision_digest: digest(0xbb),
                    algorithm: GatewayRateShapingAlgorithm::TokenBucket(
                        GatewayRateShapingTokenBucket {
                            capacity: 120,
                            refill_tokens_per_second: 60,
                        },
                    ),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("command bus")
            .expect("register");
        assert_eq!(result.profile.profile_id, "public-api-default");
        assert!(catalog.find_by_profile_id("public-api-default").is_some());
        let durable = store.list_all().await.expect("list");
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].policy_revision_digest, digest(0xbb));
    }

    #[tokio::test]
    async fn rejects_invalid_profile_before_catalog_write() {
        let catalog = InMemoryGatewayRateShapingProfileCatalog::empty();
        let store = InMemoryGatewayRateShapingProfileDurableStore::empty();
        let handler = RegisterGatewayRateShapingProfileHandler::new(
            Arc::clone(&catalog) as Arc<dyn IGatewayRateShapingProfileCatalog>,
            Arc::clone(&store) as Arc<dyn IGatewayRateShapingProfileDurableStore>,
        );
        let error = handler
            .execute(
                RegisterGatewayRateShapingProfile {
                    profile_id: "".into(),
                    policy_revision_digest: digest(0xbb),
                    algorithm: GatewayRateShapingAlgorithm::TokenBucket(
                        GatewayRateShapingTokenBucket {
                            capacity: 1,
                            refill_tokens_per_second: 1,
                        },
                    ),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("command bus")
            .expect_err("invalid");
        assert!(matches!(error, ApplicationError::Invalid(_)));
        assert!(catalog.find_by_profile_id("").is_none());
        assert!(store.list_all().await.expect("list").is_empty());
    }
}
