use core::{sync::atomic, convert::Infallible};
use embedded_dma::ReadBuffer;

use crate::hal;
use crate::utils::InfallibleResult;
use super::dma;

type DmaChannel = dma::DmaChannel<5>;

/// TX only, asynchronious SPI implementation
///
/// Implementation that uses SPI2 to just send arbitrary data.
/// MISO/SCK pins are not used.
pub struct SpiTx {
    spi: hal::pac::SPI2,
    dma: DmaChannel,
}

#[derive(Debug)]
pub struct TransferOngoing;

impl SpiTx {
    pub fn new<MOSIPIN, F>(
        spi: hal::pac::SPI2,
        dma: DmaChannel,
        _mosi: MOSIPIN,
        freq: F,
        rcc: &mut hal::rcc::Rcc,
    ) -> Self
    where
        MOSIPIN: hal::spi::MosiPin<hal::pac::SPI2>,
        F: Into<hal::time::Hertz>
    {
        // Need to access some registers outside of HAL type system (field `regs` is private)
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        // Enable SPI clock & reset it
        rcc_regs.apb1enr.modify(|_, w| w.spi2en().enabled());
        rcc_regs.apb1rstr.modify(|_, w| w.spi2rst().set_bit());
        rcc_regs.apb1rstr.modify(|_, w| w.spi2rst().clear_bit());

        // Enable DMA clock
        rcc_regs.ahbenr.modify(|_, w| w.dmaen().enabled());

        let mut s = Self { spi, dma };

        // Disable SPI & DMA
        s.spi.cr1.modify(|_, w| w.spe().disabled());
        s.dma.ch().cr.modify(|_, w| w.en().disabled());

        // Calculate baud rate
        let br = Self::get_baudrate_divisor(rcc.clocks.pclk().0, freq.into().0);

        // Ignore CPHA/CPOL as we don't even use clock
        s.spi.cr1.write(|w|  {
            w
                .br().bits(br)
                .lsbfirst().msbfirst()
                .crcen().disabled()
                .mstr().master()
                // software slave management, must use "not selected" or won't send anything!
                .ssm().enabled()
                .ssi().slave_not_selected()
                // transmit-only using half-duplex settings (could use full-duplex too)
                .bidimode().bidirectional()
                .bidioe().output_enabled()
                .rxonly().full_duplex()
        });

        s.spi.cr2.write(|w| {
            w
                .ssoe().disabled()
                // TODO: 16-bit could potentially be faster (less memory operations), with dma 16->16
                .ds().eight_bit()
                .ldma_tx().even()
                .txdmaen().disabled()  // enabled later to trigger transfer
        });

        s.dma.ch().cr.write(|w| {
            w
                .dir().from_memory()
                .mem2mem().disabled()
                .circ().disabled()
                .minc().enabled()
                .pinc().disabled()
                .msize().bits8()
                .psize().bits8()
                .pl().high()  // TODO: decide on priority
                .htie().disabled()
                .teie().enabled()
                .tcie().enabled()
        });

        s.spi.cr1.modify(|_, w| w.spe().enabled());

        // Do NOT enable SPI (see RM0091; SPI functional description; Communication using DMA)
        s
    }

    fn get_baudrate_divisor(pclk: u32, freq: u32) -> u8 {
        // Be exact, else panic
        match (pclk / freq, pclk % freq) {
            (_, rem) if rem != 0 => panic!("Unreachable SPI frequency"),
            (2, _) => 0b000,
            (4, _) => 0b001,
            (8, _) => 0b010,
            (16, _) => 0b011,
            (32, _) => 0b100,
            (64, _) => 0b101,
            (128, _) => 0b110,
            (256, _) => 0b111,
            _ => panic!("SPI clock divider not available"),
        }
    }

    pub fn with_buf<BUF>(self, buf: BUF) -> SpiTransfer<BUF>
    where
        BUF: ReadBuffer<Word = u8>
    {
        SpiTransfer::init(self, buf)
    }
}

pub struct SpiTransfer<BUF> {
    tx: SpiTx,
    buf: BUF,
    ready: bool,
}

impl<BUF> SpiTransfer<BUF>
where
    BUF: ReadBuffer<Word = u8>
{
    fn init(mut spi: SpiTx, buf: BUF) -> Self {
        // Configure channel
        let (src, len) = unsafe { buf.read_buffer() };
        let dst = spi.spi.dr.as_ptr() as u32;
        spi.dma.ch().mar.write(|w| unsafe { w.ma().bits(src as u32) });
        spi.dma.ch().par.write(|w| unsafe { w.pa().bits(dst) });
        spi.dma.ch().ndtr.write(|w| w.ndt().bits(len as u16));

        Self { tx: spi, buf, ready: true }
    }

    /// Start DMA transfer
    ///
    ///
    pub fn start(&mut self) -> nb::Result<(), TransferOngoing> {
        if !self.ready {
            return Err(nb::Error::Other(TransferOngoing));
        }

        // Wait for any data from previous transfer that has not been transmitted yet
        // Maybe it's not even needed, because DMA should just wait for space in FIFO,
        // but in practice SPI will most likely be ready anyway, so leave it for now.
        match self.wait_spi() {
            Err(nb::Error::WouldBlock) => return Err(nb::Error::WouldBlock),
            Err(nb::Error::Other(e)) => Err(e).infallible(),
            Ok(()) => {},
        };

        self.ready = false;

        // "Preceding reads and writes cannot be moved past subsequent writes"
        atomic::compiler_fence(atomic::Ordering::Release);

        // reload buffer length
        let (_, len) = unsafe { self.buf.read_buffer() };
        self.tx.dma.ch().ndtr.write(|w| w.ndt().bits(len as u16));

        // Enable channel, then trigger DMA request
        self.tx.dma.ch().cr.modify(|_, w| w.en().enabled());
        self.tx.spi.cr2.modify(|_, w| w.txdmaen().enabled());

        Ok(())
    }

    // This may be needed if we ever want to disable SPI peripheral
    fn wait_spi(&self) -> nb::Result<(), Infallible> {
        // Wait until all data has been transmitted
        if !self.tx.spi.sr.read().ftlvl().is_empty() || self.tx.spi.sr.read().bsy().is_busy() {
            Err(nb::Error::WouldBlock)
        } else {
            Ok(())
        }
    }

    /// Handle DMA interrupt
    ///
    /// Retuns `true` if there was an interrupt - this way it is possible to
    /// call this function along handlers for other DMA channels. Error is
    /// returned if the DMA transfer error flag is on.
    pub fn on_dma_interrupt(&mut self) -> Option<Result<(), ()>> {
        self.tx.dma.handle_interrupt(dma::Interrupt::FullTransfer)
            .map(|status| {
                // Disable DMA request and channel
                self.tx.spi.cr2.modify(|_, w| w.txdmaen().disabled());
                self.tx.dma.ch().cr.modify(|_, w| w.en().disabled());

                // "Subsequent reads and writes cannot be moved ahead of preceding reads"
                atomic::compiler_fence(atomic::Ordering::Acquire);

                if status.is_ok() {
                    assert!(!self.ready, "Transfer completion but transfer have not been started");
                    self.ready = true;
                }

                status
            })
    }

    pub fn take(&mut self) -> Result<&mut BUF, TransferOngoing> {
        match self.ready {
            true => Ok(&mut self.buf),
            false => Err(TransferOngoing),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baudrate_exact() {
        let br = SpiTx::get_baudrate_divisor;
        assert_eq!(br(48_000_000, 3_000_000), 0b011); // fPCLK/16
        assert_eq!(br(48_000_000, 1_500_000), 0b100); // fPCLK/32
        assert_eq!(br(24_000_000, 3_000_000), 0b010); // fPCLK/8
        assert_eq!(br(24_000_000, 12_000_000), 0b000); // fPCLK/2
    }


    #[test]
    #[should_panic(expected = "SPI clock divider not available")]
    fn baudrate_approx() {
        SpiTx::get_baudrate_divisor(48_000_000, 2_000_000);
    }

    #[test]
    #[should_panic(expected = "Unreachable")]
    fn baudrate_unreachable() {
        SpiTx::get_baudrate_divisor(48_000_000, 3_500_000);
    }
}

