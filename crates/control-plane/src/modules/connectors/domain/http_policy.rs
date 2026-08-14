#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl ConnectorHttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectorStatusDisposition {
    Accepted,
    Retryable,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorHttpStatusPolicy {
    accepted: Vec<(u16, u16)>,
    retryable: Vec<(u16, u16)>,
}

impl ConnectorHttpStatusPolicy {
    pub fn new(
        mut accepted: Vec<(u16, u16)>,
        mut retryable: Vec<(u16, u16)>,
    ) -> Result<Self, String> {
        accepted.sort_unstable();
        retryable.sort_unstable();
        if accepted.is_empty()
            || accepted.len() > 16
            || retryable.len() > 16
            || !valid_ranges(&accepted)
            || !valid_ranges(&retryable)
            || accepted
                .iter()
                .any(|(start, end)| !(200..=299).contains(start) || !(200..=299).contains(end))
            || ranges_overlap(&accepted, &retryable)
        {
            return Err("connector HTTP status policy is invalid or overlapping".into());
        }
        Ok(Self {
            accepted,
            retryable,
        })
    }

    pub fn standard_webhook() -> Self {
        Self::new(
            vec![(200, 299)],
            vec![(408, 408), (425, 425), (429, 429), (500, 599)],
        )
        .expect("static connector status policy")
    }

    pub(crate) fn classify(&self, status: u16) -> ConnectorStatusDisposition {
        if contains(&self.accepted, status) {
            ConnectorStatusDisposition::Accepted
        } else if contains(&self.retryable, status) {
            ConnectorStatusDisposition::Retryable
        } else {
            ConnectorStatusDisposition::Rejected
        }
    }
}

fn valid_ranges(ranges: &[(u16, u16)]) -> bool {
    ranges.iter().all(|(start, end)| {
        (100..=599).contains(start) && start <= end && (100..=599).contains(end)
    }) && ranges.windows(2).all(|pair| pair[0].1 < pair[1].0)
}

fn ranges_overlap(left: &[(u16, u16)], right: &[(u16, u16)]) -> bool {
    left.iter().any(|(left_start, left_end)| {
        right
            .iter()
            .any(|(right_start, right_end)| left_start <= right_end && right_start <= left_end)
    })
}

fn contains(ranges: &[(u16, u16)], status: u16) -> bool {
    ranges
        .iter()
        .any(|(start, end)| (*start..=*end).contains(&status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_policy_is_closed_and_non_overlapping() {
        assert!(ConnectorHttpStatusPolicy::new(vec![(200, 299)], vec![(299, 500)]).is_err());
        assert!(ConnectorHttpStatusPolicy::new(vec![(99, 200)], Vec::new()).is_err());
        let policy = ConnectorHttpStatusPolicy::standard_webhook();
        assert_eq!(policy.classify(204), ConnectorStatusDisposition::Accepted);
        assert_eq!(policy.classify(429), ConnectorStatusDisposition::Retryable);
        assert_eq!(policy.classify(302), ConnectorStatusDisposition::Rejected);
    }
}
