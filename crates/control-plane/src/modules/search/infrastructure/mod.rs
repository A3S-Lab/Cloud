mod persistence;

#[cfg(test)]
pub(crate) use persistence::InMemorySearchRepository;
pub(in crate::modules::search) use persistence::PostgresSearchRepository;
