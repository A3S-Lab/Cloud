mod in_memory;
mod postgres;

pub use in_memory::InMemoryEdgeRepository;
pub use postgres::PostgresEdgeRepository;

#[cfg(test)]
mod tests;
