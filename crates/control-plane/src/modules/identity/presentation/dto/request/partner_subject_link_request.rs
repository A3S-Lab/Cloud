use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkPartnerSubjectRequest {
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
    pub principal_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvePartnerSubjectQuery {
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListPartnerSubjectLinksQuery {
    pub principal_id: Uuid,
    pub provider_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokePartnerSubjectLinkRequest {
    pub provider_key: String,
    pub issuer: String,
    pub subject: String,
    pub expected_version: u64,
}
