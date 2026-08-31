mod postgres;
mod postgres_schema;

pub(in crate::modules::search) use postgres::PostgresSearchRepository;

#[cfg(test)]
mod in_memory;
#[cfg(test)]
pub(crate) use in_memory::InMemorySearchRepository;
#[cfg(test)]
mod postgres_typed_orm_tests;
