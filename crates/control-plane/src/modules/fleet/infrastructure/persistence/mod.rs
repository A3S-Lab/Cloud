mod in_memory;
mod in_memory_control;
mod postgres;
#[cfg(test)]
mod tests;

pub use in_memory::InMemoryNodeRepository;
pub use postgres::PostgresNodeRepository;
