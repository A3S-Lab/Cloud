const MAXIMUM_QUERY_CHARACTERS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery(String);

impl SearchQuery {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > MAXIMUM_QUERY_CHARACTERS
            || trimmed.contains(['\0', '\r', '\n'])
        {
            return Err(format!(
                "search query must contain 1 to {MAXIMUM_QUERY_CHARACTERS} safe characters"
            ));
        }
        Ok(Self(trimmed.to_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_bounds_search_queries() {
        assert_eq!(
            SearchQuery::parse("  ClOuD  ").expect("query").as_str(),
            "cloud"
        );
        assert!(SearchQuery::parse("  ").is_err());
        assert!(SearchQuery::parse("a".repeat(129)).is_err());
        assert!(SearchQuery::parse("cloud\nworker").is_err());
    }
}
