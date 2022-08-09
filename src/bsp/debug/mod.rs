/// Safer interface that allows to use GPIOs or Serial
pub mod pins;

/// Raw interface better suited for tracing execution of RTIC tasks
pub mod tasks;

use core::mem::MaybeUninit;
use static_assertions as sa;

#[allow(dead_code)]
mod types {
    use crate::hal::{self, gpio};

    pub type Uart = hal::pac::USART2;
    pub type Tx = gpio::gpioa::PA2<gpio::Alternate<gpio::AF1>>;
    pub type Rx = gpio::gpioa::PA3<gpio::Alternate<gpio::AF1>>;
    pub type Serial = hal::serial::Serial<Uart, Tx, Rx>;

    pub type TxPin = gpio::gpioa::PA2<gpio::Output<gpio::PushPull>>;
    pub type RxPin = gpio::gpioa::PA3<gpio::Output<gpio::PushPull>>;
    pub type Pin = gpio::Pin<gpio::Output<gpio::PushPull>>;
}

// Get the 0-sized pin structures, as there is no data this should be safe?
unsafe fn get_tx() -> types::TxPin {
    sa::const_assert_eq!(core::mem::size_of::<types::TxPin>(), 0);
    MaybeUninit::uninit().assume_init()
}
unsafe fn get_rx() -> types::RxPin {
    sa::const_assert_eq!(core::mem::size_of::<types::RxPin>(), 0);
    MaybeUninit::uninit().assume_init()
}

/// Calculates current stack usage
///
/// # Note
///
/// This assumes that we use flip-link (which makes the stack
/// start at RAM origin) and that RAM origin is at 0x2000_0000.
#[inline(always)]
pub fn stack_usage() -> u32 {
    const RAM_ORIGIN: u32 = 0x2000_0000;
    let msp = cortex_m::register::msp::read();
    msp - RAM_ORIGIN
}

/// Helper macro for debugging a set of MCU registers
///
/// Uses [`defmt::println`] to pretty-print 32-bit registers in binary format splitting
/// nibbles/bytes using underscores for easier reading. Use as:
///
/// ```no_run
/// # #[macro_use] extern crate ghanima;
/// # use stm32f0xx_hal as hal;
/// let dma = unsafe { &*hal::pac::DMA1::ptr() };
/// let usart = unsafe { &*hal::pac::USART1::ptr() };
/// debug_regs! {
///     USART1: isr cr1 cr2 cr3 brr,
///     DMA1: isr ch2.cr ch3.cr ch5.cr,
/// };
/// ```
// FIXME: find way to use interned strings (=istr), stringify!/concat! do not work
#[macro_export]
macro_rules! debug_regs {
    ( $( $periph:ident: $( $($reg:ident).+ )+ ),+ $(,)? ) => {
        $( debug_regs! { @periph $periph $($($reg).+)+ } )+
    };
    ( @periph $periph:ident $( $($reg:ident).+ )+ ) => {
        let periph = unsafe { &*hal::pac::$periph::ptr() };
        $(
            defmt::println!(
                "{0=str}: {1=28..32:08b}_{1=24..28:08b}__{1=20..24:08b}_{1=16..20:08b}__{1=12..16:08b}_{1=8..12:08b}__{1=4..8:08b}_{1=0..4:08b}",
                concat!(
                    stringify!($periph),
                    $(
                        ".",
                        stringify!($reg)
                    ),+
                ),
                periph. $($reg).+ .read().bits()
            );
        )+
    };
}
