use stm32f0xx_hal as hal;

/// TX only, asynchronious SPI implementation
///
/// Implementation that uses SPI2 to just send arbitrary data.
/// MISO/SCK pins are not used.
pub struct Spi2Tx {
    spi: hal::pac::SPI2,
}

impl Spi2Tx {
    fn dma() -> &'static hal::pac::dma1::RegisterBlock {
        unsafe { &*hal::pac::DMA1::ptr() }
    }

    // NOTE: Using DMA channel 5. Could use channel 7 if remapped in SYSCFG_CFGR1,
    // but assuming it is not recmapped!
    fn dma_channel() -> &'static hal::pac::dma1::CH {
        unsafe { &(&*hal::pac::DMA1::ptr()).ch5 }
    }

    /// Transfer given data using DMA
    pub fn transfer(&self, data: &'static [u8]) {
        let channel = Self::dma_channel();

        // Disable channel & SPI
        channel.cr.modify(|_, w| w.en().disabled());

        // Configure channel
        let src = data.as_ptr() as u32;
        let dst = self.spi.dr.as_ptr() as u32;
        let n = data.len() as u16;
        channel.mar.write(|w| unsafe { w.ma().bits(src) });
        channel.par.write(|w| unsafe { w.pa().bits(dst) });
        channel.ndtr.write(|w| w.ndt().bits(n));

        // Enable channel, then SPI
        channel.cr.modify(|_, w| w.en().enabled());
        self.spi.cr1.modify(|_, w| w.spe().enabled());
    }

    pub fn finish(&self) -> Result<(), ()> {
        // TODO: handle error flag separately
        let isr = Self::dma().isr.read();
        if isr.tcif5().is_not_complete() || isr.teif5().is_error() {
            return Err(())
        }

        // Clear all interrupt flags
        Self::dma().ifcr.write(|w| w.cgif5().set_bit());

        // Wait until all data has been transmitted
        // TODO: could we avoid that by never disabling SPI? (it should keep consuming FIFO)
        while !self.spi.sr.read().ftlvl().is_empty() {}
        while self.spi.sr.read().bsy().is_busy() {}

        // Disable channel, then SPI
        Self::dma_channel().cr.modify(|_, w| w.en().disabled());
        self.spi.cr1.modify(|_, w| w.spe().disabled());

        Ok(())
    }

    pub fn new<MOSIPIN>(
            spi: hal::pac::SPI2,
            _mosi: MOSIPIN,
        ) -> Self
        where MOSIPIN: hal::spi::MosiPin<hal::pac::SPI2>
    {
        // Need to access some registers outside of HAL type system
        let rcc = unsafe { &*hal::pac::RCC::ptr() };
        let dma_channel = Self::dma_channel();

        // Enable SPI clock & reset it
        rcc.apb1enr.modify(|_, w| w.spi2en().enabled());
        rcc.apb1rstr.modify(|_, w| w.spi2rst().set_bit());
        rcc.apb1rstr.modify(|_, w| w.spi2rst().clear_bit());

        // Enable DMA clock
        rcc.ahbenr.modify(|_, w| w.dmaen().enabled());

        // Disable SPI & DMA
        spi.cr1.modify(|_, w| w.spe().disabled());
        dma_channel.cr.modify(|_, w| w.en().disabled());

        // TODO: Calculate baud rate

        // Ignore CPHA/CPOL as we don't even use clock
        spi.cr1.write(|w|  {
            w
                .br().div2()
                .lsbfirst().msbfirst()
                .crcen().disabled()
                .mstr().master()
                .ssm().enabled()
                .ssi().slave_selected()
                // transmit-only: full-duplex, we just ignore input data (or use half-duplex?)
                .rxonly().full_duplex()
                .bidimode().bidirectional()
                .bidioe().output_enabled()
        });

        spi.cr2.write(|w| {
            w
                // TODO: 16-bit could potentially be faster (less memory operations), with dma 16->16
                .ds().eight_bit()
                .ssoe().disabled()
                .txdmaen().enabled()
                .ldma_tx().even()
        });

        dma_channel.cr.write(|w| {
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
                .tcie().enabled()  // TODO: which interrupts we want? (error?)
        });

        // Do NOT enable SPI (see RM0091; SPI functional description; Communication using DMA)

        Self { spi }
    }
}
