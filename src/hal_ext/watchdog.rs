use crate::hal;

/// System window watchdog - WWDG
pub struct WindowWatchdog {
    wwdg: hal::pac::WWDG,
    params: WindowParams,
}

/// Parameters for WWDG configuration
pub struct WindowParams {
    counter: u8,
    window: u8,
}

/// Control system reset reason flags
pub mod reset_flags {
    use super::hal;

    /// Clear all reset flags
    pub fn clear(_rcc: &mut hal::rcc::Rcc) {
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };
        rcc_regs.csr.write(|w| w.rmvf().set_bit());
    }

    /// Check if last system reset was due to window watchdog
    pub fn was_window_watchdog(_rcc: &mut hal::rcc::Rcc) -> bool {
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };
        rcc_regs.csr.read().wwdgrstf().bit_is_set()
    }
}

impl WindowWatchdog {
    /// Create watchdog instance, must be started using [`Self::start`]
    pub fn new(
        wwdg: hal::pac::WWDG,
        params: WindowParams,
    ) -> Self {
        Self { wwdg, params }
    }

    /// Configure and enable window watchdog
    ///
    /// During each period the watchdog must be fed at time T inside the
    /// configured time window: `(period_ms - window_ms) < T < period_ms`
    /// (where t=0 is the last feeding time).
    ///
    /// With f_PCLK = 24 MHz the maximum time period is around 87 ms,
    /// while with f_PCLK = 48 MHz it is around 43 ms.
    pub fn start(&mut self, _rcc: &mut hal::rcc::Rcc) {
        // Need to access some registers outside of HAL type system (field `regs` is private)
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        rcc_regs.apb1enr.modify(|_, w| w.wwdgen().enabled());
        rcc_regs.apb1rstr.modify(|_, w| w.wwdgrst().set_bit());
        rcc_regs.apb1rstr.modify(|_, w| w.wwdgrst().clear_bit());

        self.wwdg.cfr.write(|w| w
                .wdgtb().div8()
                .w().bits(self.params.window));

        self.wwdg.cr.write(|w| w
            .wdga().enabled()
            .t().bits(self.params.counter));
    }

    /// Stop window watchdog when the core is halted during MCU debugging
    pub fn stop_on_debug(&mut self, stop: bool, dbg: &mut hal::pac::DBGMCU, _rcc: &mut hal::rcc::Rcc) {
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };
        rcc_regs.apb2enr.modify(|_, w| w.dbgmcuen().enabled());

        dbg.apb1_fz.modify(|_, w| w.dbg_wwdg_stop().bit(stop));
    }

    /// Feed the watchdog, must be done during the configured time window
    pub fn feed(&mut self) {
        // No need to set WDGA, it's just not possible to disable the watchdog.
        self.wwdg.cr.write(|w| w.t().bits(self.params.counter));
    }

    /// Check if we are in the feeding window
    pub fn ready(&mut self) -> bool {
        self.wwdg.cr.read().t().bits() < self.wwdg.cfr.read().w().bits()
    }

    /// Feed the watchdog if we are in the window
    pub fn maybe_feed(&mut self) -> bool {
        let ready = self.ready();
        if ready {
            self.feed();
        }
        ready
    }
}

impl WindowParams {
    /// Pre-calculate window watchdog parameters
    ///
    /// Computes watchdog parameters given fPCLK and window times. In each watchdog period
    /// it must be fed at T such that `window_start_us` < T `window_end_us`. The accuracy
    /// of calculations is around 1 watchdog clock tick (clock frequency = fPCLK/4096/8).
    /// Maximum watchdog period at fPCLK=24 MHz is ~87 ms and at fPCLK=48 MHz it is ~43 ms.
    ///
    /// # Panics
    ///
    /// When the resulting computed parameters are incorrect, i.e. are out of range (> 127),
    /// would result in immediate reset (< 64) or window start would be after window end.
    ///
    /// Make sure to use this function to compute a `const` value, e.g.
    ///
    /// ```rust
    /// # use ghanima::hal_ext::watchdog::{WindowWatchdog, WindowParams};
    /// const PARAMS: WindowParams = WindowParams::new(24_000_000, 30_000, 70_000);
    /// ```
    ///
    /// This way assertions in this function will result in compile-time errors.
    pub const fn new(
        f_pclk_hz: u32,
        window_start_us: u32,
        window_end_us: u32,
    ) -> Self {
        // Watchdog clock frequency = fpclk/4096/div, for now we always use div8
        // fpclk(48 MHz) => 682.6 us, fpclk(24 MHz) => 1365.3 us
        let tick_ns = 1_000_000_000_u64 * 4096 * 8 / f_pclk_hz as u64;

        assert!(window_start_us < window_end_us);

        let counter = 0x40 + (window_end_us as u64 * 1000) / tick_ns;
        let window = counter - ((window_end_us - window_start_us) as u64 * 1000) / tick_ns;

        assert!(counter >= 0x40 && counter < 0x7f);
        assert!(window >= 0x40 && window < 0x7f);
        assert!(window < counter);

        let counter = counter as u8;
        let window = window as u8;

        Self { counter, window }
    }
}
