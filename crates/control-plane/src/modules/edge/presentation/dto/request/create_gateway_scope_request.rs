use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateGatewayScopeRequest {
    #[serde(default)]
    pub node_id: Option<Uuid>,
    #[serde(default)]
    pub node_ids: Vec<Uuid>,
    #[serde(default)]
    pub min_ready: Option<u32>,
    #[serde(default)]
    pub max_unavailable: Option<u32>,
}

impl CreateGatewayScopeRequest {
    pub fn members(self) -> Result<(Uuid, Vec<Uuid>, u32, u32), String> {
        let member_node_ids = match (self.node_id, self.node_ids.is_empty()) {
            (Some(node_id), true) => vec![node_id],
            (None, false) => self.node_ids,
            (Some(_), false) => {
                return Err("provide either nodeId or nodeIds, not both".into());
            }
            (None, true) => {
                return Err("nodeId or nodeIds is required".into());
            }
        };
        let primary_node_id = member_node_ids[0];
        Ok((
            primary_node_id,
            member_node_ids,
            self.min_ready.unwrap_or(1),
            self.max_unavailable.unwrap_or(0),
        ))
    }
}
