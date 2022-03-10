use core::sync::atomic::{AtomicBool, Ordering};
use hal::prelude::*;
use cortex_m::interrupt;
use crate::{hal, utils::InfallibleResult};
use super::{types::*, get_tx, get_rx};

#[cfg(all(feature = "idle_sleep", feature = "debug_tasks"))]
compile_error!("debug_tasks will not work with idle_sleep enabled");

/// Grant GPIOs to this module
pub fn init((tx, rx): (Tx, Rx)) {
    interrupt::free(|cs| {
        tx.into_push_pull_output_hs(cs);
        rx.into_push_pull_output_hs(cs);
        INIT.store(true, Ordering::SeqCst);
    })
}

static INIT: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn ensure_init() {
    if !INIT.load(Ordering::SeqCst) {
        panic!("init() never called");
    }
}

/// Use a trace-dedicated GPIO pin to trace execution of code
pub mod trace {
    use super::*;

    /// Set trace GPIO pin as high
    #[inline(always)]
    pub fn start() {
        if cfg!(feature = "debug_tasks") {
            ensure_init();
            trace_pin().set_low().infallible();
            trace_pin().set_high().infallible();
        }
    }

    /// Set trace GPIO pin as low
    #[inline(always)]
    pub fn end() {
        if cfg!(feature = "debug_tasks") {
            ensure_init();
            trace_pin().set_high().infallible();
            trace_pin().set_low().infallible();
        }
    }

    /// Run code with trace GPIO pin high
    #[inline(always)]
    pub fn run<F, T>(f: F) -> T
    where
        F: FnOnce() -> T
    {
        start();
        let result = f();
        end();
        result
    }
}

/// Use task-dedicated GPIO pin to trace execution of tasks
pub mod task {
    use super::*;

    static PENDING: AtomicBool = AtomicBool::new(false);

    /// To be called on task enter
    #[inline(always)]
    pub fn enter() {
        if cfg!(feature = "debug_tasks") {
            ensure_init();
            PENDING.store(true, Ordering::SeqCst);
            // Make sure to always have 0-to-1 transition so that when another
            // task preempts this one it will be visible as 111101111...
            task_pin().set_low().infallible();
            task_pin().set_high().infallible();
        }
    }

    /// To be called on all task exit points
    #[inline(always)]
    pub fn exit() {
        if cfg!(feature = "debug_tasks") {
            ensure_init();
            // Make sure to have a 1-to-0 transition.
            task_pin().set_high().infallible();
            task_pin().set_low().infallible();
            // If there is any task pending then set pin back to 1.
            if PENDING.load(Ordering::SeqCst) {
                task_pin().set_high().infallible();
            }
        }
    }

    /// Call in idle task to indicate that no tasks are pending
    #[inline(always)]
    pub fn idle() {
        if cfg!(feature = "debug_tasks") {
            task_pin().set_low().infallible();
            PENDING.store(false, Ordering::SeqCst);
        }
    }
}


#[inline(always)]
fn trace_pin() -> Pin {
    unsafe { get_tx().downgrade() }
}

#[inline(always)]
fn task_pin() -> Pin {
    unsafe { get_rx().downgrade() }
}
