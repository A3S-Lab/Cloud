mod in_memory;
mod postgres;
mod postgres_acknowledgement;
mod postgres_certificate_convergence;
mod postgres_cutovers;
mod postgres_gateway_scopes;
mod postgres_mcp_credentials;
mod postgres_mcp_route_policies;
mod postgres_rollout_routes;
mod postgres_rollouts;
mod postgres_schema;
mod postgres_tls;
#[cfg(test)]
mod postgres_typed_orm_tests;

pub use in_memory::InMemoryEdgeRepository;
pub use postgres::PostgresEdgeRepository;

#[cfg(test)]
mod tests;
