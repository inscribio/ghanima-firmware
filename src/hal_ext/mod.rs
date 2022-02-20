//! Hardware Abstraction Layer
//!
//! This module is an extension to [`stm32f0xx_hal`] that covers some more
//! project-specific hardware - mainly DMA abstractions.

/// CRC peripheral
pub mod crc;
/// DMA HAL for stm32f0
pub mod dma;
/// Rebooting to embedded bootloader
pub mod reboot;
/// TX only SPI with DMA
pub mod spi;
/// UART with DMA
pub mod uart;

mod checksum;
mod circ_buf;

pub use checksum::{ChecksumGen, ChecksumEncoder};

#[cfg(test)]
pub use checksum::mock as checksum_mock;
