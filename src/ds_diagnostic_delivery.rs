//! Session-local admission and bounded shutdown; no locks or I/O in producers.
use std::sync::atomic::{
    AtomicBool, AtomicU64, AtomicUsize,
    Ordering::{AcqRel, Acquire, Relaxed, Release},
};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const CLOSED: usize = 1usize << (usize::BITS - 1);
pub(super) const DRAIN_LIMIT: Duration = Duration::from_millis(150);

#[derive(Default)]
pub(super) struct Delivery {
    admission: AtomicUsize,
    pub queued: AtomicUsize,
    rejected_after_close: AtomicU64,
    close_started: OnceLock<Instant>,
    pub ended: AtomicBool,
}
pub(super) struct Permit<'a> {
    delivery: &'a Delivery,
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.delivery.admission.fetch_sub(1, Release);
    }
}
impl Delivery {
    pub fn enter(&self) -> Option<Permit<'_>> {
        // One atomic linearizes admission against close. Rejected arrivals do
        // not affect the in-flight count, so a drained queue stays drained.
        if self
            .admission
            .fetch_update(AcqRel, Acquire, |state| {
                if state < CLOSED - 1 {
                    Some(state + 1)
                } else {
                    None
                }
            })
            .is_err()
        {
            self.rejected_after_close.fetch_add(1, Relaxed);
            None
        } else {
            Some(Permit { delivery: self })
        }
    }
    pub fn close(&self) {
        self.close_started.get_or_init(Instant::now);
        self.admission.fetch_or(CLOSED, AcqRel);
    }
    pub fn closed(&self) -> bool {
        self.admission.load(Acquire) & CLOSED != 0
    }
    pub fn inflight(&self) -> usize {
        self.admission.load(Acquire) & !CLOSED
    }
    pub fn drained(&self) -> bool {
        self.admission.load(Acquire) == CLOSED && self.queued.load(Acquire) == 0
    }
    pub fn timed_out(&self) -> bool {
        self.closed()
            && self
                .close_started
                .get()
                .is_some_and(|t| t.elapsed() >= DRAIN_LIMIT)
    }
    pub fn rejected(&self) -> u64 {
        self.rejected_after_close.load(Relaxed)
    }
    pub fn closed_at(&self) -> Option<Instant> {
        self.close_started.get().copied()
    }
}
