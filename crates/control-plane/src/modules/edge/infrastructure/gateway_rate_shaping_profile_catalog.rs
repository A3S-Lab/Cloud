//! In-memory Gateway rate-shaping profile catalog and Edge admission adapter.
//!
//! Production may start empty and fail closed until profiles are registered.
//! Tests seed profiles explicitly.

use crate::modules::edge::application::{
    ApplicationPublicationRateShapingBindingAdmissionRequest,
    ApplicationPublicationRateShapingBoundProfile,
    IApplicationPublicationRateShapingBindingAdmissionPort, RATE_SHAPING_BINDING_INVALID,
};
use crate::modules::edge::domain::GatewayRateShapingProfile;
use crate::modules::shared_kernel::domain::Sha256Digest;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Lookup + registration surface for Gateway-owned rate-shaping profile revisions.
pub trait IGatewayRateShapingProfileCatalog: Send + Sync {
    /// Resolve by profile id only so callers can distinguish unknown vs digest mismatch.
    fn find_by_profile_id(&self, profile_id: &str) -> Option<GatewayRateShapingProfile>;

    /// Insert or replace one validated profile revision (process-local until a durable owner exists).
    fn register(&self, profile: GatewayRateShapingProfile) -> Result<(), String>;
}

/// Process-local catalog; empty by default (fail closed until register).
#[derive(Debug, Default)]
pub struct InMemoryGatewayRateShapingProfileCatalog {
    profiles: RwLock<HashMap<String, GatewayRateShapingProfile>>,
}

impl InMemoryGatewayRateShapingProfileCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn empty() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Insert or replace one validated profile revision.
    pub fn register(&self, profile: GatewayRateShapingProfile) -> Result<(), String> {
        IGatewayRateShapingProfileCatalog::register(self, profile)
    }
}

impl IGatewayRateShapingProfileCatalog for InMemoryGatewayRateShapingProfileCatalog {
    fn find_by_profile_id(&self, profile_id: &str) -> Option<GatewayRateShapingProfile> {
        self.profiles
            .read()
            .ok()
            .and_then(|guard| guard.get(profile_id).cloned())
    }

    fn register(&self, profile: GatewayRateShapingProfile) -> Result<(), String> {
        profile.validate()?;
        let mut guard = self
            .profiles
            .write()
            .map_err(|_| "gateway rate shaping catalog lock poisoned".to_owned())?;
        guard.insert(profile.profile_id.clone(), profile);
        Ok(())
    }
}

/// Edge adapter: admit declare-only publication rate refs against the Gateway catalog.
#[derive(Clone)]
pub struct EdgeApplicationPublicationRateShapingBindingAdmissionAdapter {
    catalog: Arc<dyn IGatewayRateShapingProfileCatalog>,
}

impl EdgeApplicationPublicationRateShapingBindingAdmissionAdapter {
    pub fn new(catalog: Arc<dyn IGatewayRateShapingProfileCatalog>) -> Self {
        Self { catalog }
    }

    pub fn empty() -> Self {
        Self::new(InMemoryGatewayRateShapingProfileCatalog::empty())
    }
}

impl IApplicationPublicationRateShapingBindingAdmissionPort
    for EdgeApplicationPublicationRateShapingBindingAdmissionAdapter
{
    fn admit(
        &self,
        request: ApplicationPublicationRateShapingBindingAdmissionRequest,
    ) -> Result<ApplicationPublicationRateShapingBoundProfile, String> {
        if request.profile_id.trim().is_empty() {
            return Err(binding_invalid("rate shaping profile_id must be non-empty"));
        }
        if let Err(error) = Sha256Digest::parse(request.policy_revision_digest.as_str()) {
            return Err(binding_invalid(error));
        }

        let Some(profile) = self.catalog.find_by_profile_id(&request.profile_id) else {
            return Err(binding_invalid(format!(
                "rate shaping profile not found: {}",
                request.profile_id
            )));
        };
        if profile.policy_revision_digest != request.policy_revision_digest {
            return Err(binding_invalid(format!(
                "rate shaping policy revision digest mismatch for profile {}",
                request.profile_id
            )));
        }
        profile.validate().map_err(binding_invalid)?;
        Ok(ApplicationPublicationRateShapingBoundProfile { profile })
    }
}

fn binding_invalid(detail: impl Into<String>) -> String {
    format!("{RATE_SHAPING_BINDING_INVALID}: {}", detail.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{
        GatewayRateShapingAlgorithm, GatewayRateShapingTokenBucket,
    };

    fn digest(byte: u8) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", format!("{:02x}", byte).repeat(32)))
            .expect("digest")
    }

    fn sample_profile(digest_byte: u8) -> GatewayRateShapingProfile {
        GatewayRateShapingProfile::new(
            "public-api-default",
            digest(digest_byte),
            GatewayRateShapingAlgorithm::TokenBucket(GatewayRateShapingTokenBucket {
                capacity: 120,
                refill_tokens_per_second: 60,
            }),
        )
        .expect("profile")
    }

    #[test]
    fn admits_matching_profile_revision() {
        let catalog = InMemoryGatewayRateShapingProfileCatalog::new();
        catalog.register(sample_profile(0xbb)).expect("register");
        let admission =
            EdgeApplicationPublicationRateShapingBindingAdmissionAdapter::new(Arc::new(catalog));
        let bound = admission
            .admit(ApplicationPublicationRateShapingBindingAdmissionRequest::new(
                "public-api-default",
                digest(0xbb),
            ))
            .expect("admit");
        assert_eq!(bound.profile.profile_id, "public-api-default");
    }

    #[test]
    fn rejects_unknown_profile_and_digest_mismatch() {
        let catalog = InMemoryGatewayRateShapingProfileCatalog::new();
        catalog.register(sample_profile(0xbb)).expect("register");
        let admission =
            EdgeApplicationPublicationRateShapingBindingAdmissionAdapter::new(Arc::new(catalog));
        let unknown = admission.admit(ApplicationPublicationRateShapingBindingAdmissionRequest::new(
            "missing",
            digest(0xbb),
        ));
        assert!(unknown
            .unwrap_err()
            .contains("rate shaping profile not found"));
        let mismatch = admission.admit(ApplicationPublicationRateShapingBindingAdmissionRequest::new(
            "public-api-default",
            digest(0xcc),
        ));
        assert!(mismatch
            .unwrap_err()
            .contains("rate shaping policy revision digest mismatch"));
    }


    #[test]
    fn seeds_multiple_profiles_for_admission() {
        let catalog = InMemoryGatewayRateShapingProfileCatalog::empty();
        catalog
            .register(sample_profile(0xbb))
            .expect("register first");
        catalog
            .register(GatewayRateShapingProfile::new(
                "burst-gcra",
                digest(0xcc),
                crate::modules::edge::domain::GatewayRateShapingAlgorithm::Gcra(
                    crate::modules::edge::domain::GatewayRateShapingGcra {
                        emission_interval_nanos: 1_000_000,
                        burst_tolerance: 10,
                    },
                ),
            )
            .expect("profile"))
            .expect("register second");
        let admission =
            EdgeApplicationPublicationRateShapingBindingAdmissionAdapter::new(Arc::clone(
                &catalog,
            ) as Arc<dyn IGatewayRateShapingProfileCatalog>);
        admission
            .admit(ApplicationPublicationRateShapingBindingAdmissionRequest::new(
                "public-api-default",
                digest(0xbb),
            ))
            .expect("admit token bucket");
        admission
            .admit(ApplicationPublicationRateShapingBindingAdmissionRequest::new(
                "burst-gcra",
                digest(0xcc),
            ))
            .expect("admit gcra");
    }
}
