//! Hardware Abstraction Layer
//!
//! This module is an extension to `stm32f0xx_hal` that covers some more
//! project-specific hardware - mainly DMA abstractions.

pub mod crc;
pub mod dma;
pub mod reboot;
pub mod spi;
pub mod uart;

mod checksum;
mod circ_buf;

pub use checksum::Checksum;
