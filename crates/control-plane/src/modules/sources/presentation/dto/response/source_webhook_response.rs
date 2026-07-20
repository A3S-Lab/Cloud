use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceWebhookResponse {
    pub received: bool,
}

impl SourceWebhookResponse {
    pub const fn received() -> Self {
        Self { received: true }
    }
}
