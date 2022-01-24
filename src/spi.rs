use core::sync::atomic;

use embedded_dma::StaticReadBuffer;
use crate::hal;

/// TX only, asynchronious SPI implementation
///
/// Implementation that uses SPI2 to just send arbitrary data.
/// MISO/SCK pins are not used.
pub struct SpiTx {
    spi: hal::pac::SPI2,
    dma: hal::pac::DMA1, // TODO: own only DMA1.ch5 and take DMA1.ifcr/isr as method arguments
}

impl SpiTx {
    // FIXME: channel is hardcoded when using IFCR/ISR
    fn dma_channel(&self) -> &hal::pac::dma1::CH {
        &self.dma.ch5
    }

    pub fn new<MOSIPIN, F>(
        spi: hal::pac::SPI2,
        dma: hal::pac::DMA1,
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

        let s = Self { spi, dma };

        // Disable SPI & DMA
        s.spi.cr1.modify(|_, w| w.spe().disabled());
        s.dma_channel().cr.modify(|_, w| w.en().disabled());

        // Calculate baud rate, be exact.
        let (pclk, f) = (rcc.clocks.pclk().0, freq.into().0);
        let br = match (pclk / f, pclk % f) {
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
        };

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

        s.dma_channel().cr.write(|w| {
            w
                .dir().from_memory()
                .mem2mem().disabled()
                .circ().disabled()
                .minc().enabled()
                .pinc().disabled()
                .msize().bits8()
                .psize().bits8()
                .pl().medium()  // TODO: decide on priority
                .htie().disabled()
                .teie().enabled()
                .tcie().enabled()
        });

        s.spi.cr1.modify(|_, w| w.spe().enabled());

        // Do NOT enable SPI (see RM0091; SPI functional description; Communication using DMA)
        s
    }

    pub fn with_buf<BUF>(self, buf: BUF) -> SpiTransfer<BUF>
    where
        BUF: StaticReadBuffer<Word = u8>
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
    BUF: StaticReadBuffer<Word = u8>
{
    pub fn init(spi: SpiTx, buf: BUF) -> Self {
        let dma_ch = spi.dma_channel();

        // Configure channel
        let (src, len) = unsafe { buf.read_buffer() };
        let dst = spi.spi.dr.as_ptr() as u32;
        dma_ch.mar.write(|w| unsafe { w.ma().bits(src as u32) });
        dma_ch.par.write(|w| unsafe { w.pa().bits(dst) });
        dma_ch.ndtr.write(|w| w.ndt().bits(len as u16));

        Self { tx: spi, buf, ready: true }
    }

    pub fn start(&mut self) {
        assert_eq!(self.ready, true);
        self.ready = false;

        // "Preceding reads and writes cannot be moved past subsequent writes"
        atomic::compiler_fence(atomic::Ordering::Release);

        // reload buffer length
        let (_, len) = unsafe { self.buf.read_buffer() };
        self.tx.dma_channel().ndtr.write(|w| w.ndt().bits(len as u16));

        // Enable channel, then trigger DMA request
        self.tx.dma_channel().cr.modify(|_, w| w.en().enabled());
        self.tx.spi.cr2.modify(|_, w| w.txdmaen().enabled());
    }

    pub fn finish(&mut self) -> Result<bool, ()> {
        let isr = self.tx.dma.isr.read();
        if isr.gif5().is_no_event() {
            // not an interrupt from our channel
            return Ok(false);
        }

        let is_err = isr.teif5().is_error();

        // Clear all interrupt flags
        self.tx.dma.ifcr.write(|w| w.cgif5().set_bit());

        // Disable DMA request and channel
        self.tx.spi.cr2.modify(|_, w| w.txdmaen().disabled());
        self.tx.dma_channel().cr.modify(|_, w| w.en().disabled());

        if is_err {
            // TODO: error handling
            return Err(());
        }

        // Wait until all data has been transmitted
        // TODO: could we avoid that by never disabling SPI? (it should keep consuming FIFO)
        while !self.tx.spi.sr.read().ftlvl().is_empty() {}
        while self.tx.spi.sr.read().bsy().is_busy() {}

        // "Subsequent reads and writes cannot be moved ahead of preceding reads"
        atomic::compiler_fence(atomic::Ordering::Acquire);

        self.ready = true;

        Ok(true)
    }

    pub fn take(&mut self) -> Result<&mut BUF, ()> {
        match self.ready {
            true => Ok(&mut self.buf),
            false => Err(()),
        }
    }
}
