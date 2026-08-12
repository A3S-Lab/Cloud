mod in_memory;
mod in_memory_control;
#[cfg(test)]
mod inventory_typed_orm_tests;
mod postgres;
#[cfg(test)]
mod tests;

pub use in_memory::InMemoryNodeRepository;
pub use postgres::PostgresNodeRepository;
pub(crate) use postgres::{node_pool_placement_is_eligible, require_current_inventory};
