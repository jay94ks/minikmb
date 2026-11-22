//! Includes live gateway/forwarding flows and accepted connections awaiting the stack.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Weak,
};
pub const MAX: usize = 32;
#[derive(Clone, Default)]
pub struct Budget(Arc<AtomicUsize>);
#[derive(Clone)]
pub struct Permit {
    _ticket: Arc<Ticket>,
}
struct Ticket(Budget);
#[derive(Clone)]
pub(super) struct Witness(Weak<Ticket>);
impl Witness {
    pub fn live(&self) -> bool {
        self.0.strong_count() != 0
    }
}
impl Permit {
    pub(super) fn witness(&self) -> Witness {
        Witness(Arc::downgrade(&self._ticket))
    }
}
impl Budget {
    pub fn used(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }
    pub fn reserve(&self) -> Option<Permit> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX).then_some(n + 1)
            })
            .ok()
            .map(|_| Permit {
                _ticket: Arc::new(Ticket(self.clone())),
            })
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.0 .0.fetch_sub(1, Ordering::AcqRel);
    }
}
