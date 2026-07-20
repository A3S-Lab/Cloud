mod in_memory;
mod postgres;
mod postgres_acknowledgement;
mod postgres_certificate_convergence;
mod postgres_cutovers;
mod postgres_tls;

pub use in_memory::InMemoryEdgeRepository;
pub use postgres::PostgresEdgeRepository;

#[cfg(test)]
mod tests;
