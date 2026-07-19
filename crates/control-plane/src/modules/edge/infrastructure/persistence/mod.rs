mod in_memory;
mod postgres;
mod postgres_tls;

pub use in_memory::InMemoryEdgeRepository;
pub use postgres::PostgresEdgeRepository;

#[cfg(test)]
mod tests;
