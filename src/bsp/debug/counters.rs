#[derive(Default)]
pub struct Counter {
    #[cfg(feature = "task-counters")]
    cnt: core::sync::atomic::AtomicU16,
}

#[cfg(feature = "task-counters")]
impl Counter {
    #[inline(always)]
    pub fn inc(&self) {
        let _ = atomic::fetch_saturating_add(&self.cnt, 1);
    }

    #[inline(always)]
    pub fn pop(&self) -> u16 {
        atomic::swap(&self.cnt, 0)
    }
}

#[cfg(not(feature = "task-counters"))]
impl Counter {
    #[inline(always)]
    pub fn inc(&self) {
    }

    #[inline(always)]
    pub fn pop(&self) -> u16 {
        0
    }
}

// ARM thumbv6 does not support atomic fetch_add so we need to use short critical sections, see:
// https://github.com/jamesmunns/bbqueue/blob/f73423c0b1c5fe04723e5b5bd57d1a44ff106473/core/src/bbbuffer.rs#L1098
mod atomic {
    use core::sync::atomic::AtomicU16;
    use core::sync::atomic::Ordering::{Acquire, Release};
    use cortex_m::interrupt::free;

    #[inline(always)]
    pub fn fetch_saturating_add(atomic: &AtomicU16, val: u16) -> u16 {
        free(|_| {
            let prev = atomic.load(Acquire);
            atomic.store(prev.saturating_add(val), Release);
            prev
        })
    }

    #[inline(always)]
    pub fn swap(atomic: &AtomicU16, val: u16) -> u16 {
        free(|_| {
            let prev = atomic.load(Acquire);
            atomic.store(val, Release);
            prev
        })
    }
}
