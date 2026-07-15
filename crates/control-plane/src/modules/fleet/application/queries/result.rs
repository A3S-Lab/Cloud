use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::NodeAvailability;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct NodeQueryResult {
    pub node: Node,
    pub availability: NodeAvailability,
}
