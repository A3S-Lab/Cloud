mod in_memory;
mod postgres;

pub use in_memory::InMemoryGatewayRoutePolicyTimelineRepository;
pub use postgres::PostgresGatewayRoutePolicyTimelineRepository;

#[cfg(test)]
mod typed_orm_tests;
