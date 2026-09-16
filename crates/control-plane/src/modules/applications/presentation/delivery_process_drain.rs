//! Process-local delivery admission drain latch (APP0.3-C8).
//!
//! This is not Fleet `NodeDrain`. It only refuses new Delivery session/invocation
//! admission while allowing observation, close, and cancel to continue.

use a3s_boot::{BootError, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared latch that marks the Delivery process as draining.
#[derive(Clone, Debug, Default)]
pub struct DeliveryProcessDrain {
    draining: Arc<AtomicBool>,
}

impl DeliveryProcessDrain {
    /// Create a non-draining latch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Latch the process into draining mode (idempotent).
    pub fn begin(&self) {
        self.draining.store(true, Ordering::SeqCst);
    }

    /// Whether new admission must be refused.
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Refuse new session/invocation admission while draining.
    pub fn refuse_new_admission(&self) -> Result<()> {
        if self.is_draining() {
            return Err(BootError::ServiceUnavailable(
                "delivery process is draining".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_new_admission_errors_only_while_draining() {
        let drain = DeliveryProcessDrain::new();
        assert!(!drain.is_draining());
        drain.refuse_new_admission().expect("admit while idle");
        drain.begin();
        assert!(drain.is_draining());
        let error = drain
            .refuse_new_admission()
            .expect_err("must refuse while draining");
        assert!(matches!(
            error,
            BootError::ServiceUnavailable(message) if message == "delivery process is draining"
        ));
        drain.begin();
        assert!(drain.is_draining());
    }
}
