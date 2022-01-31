use core::{sync::atomic, convert::Infallible};
use embedded_dma::{WriteBuffer, StaticWriteBuffer};

use crate::hal;
use hal::gpio;
use super::circ_buf::CircularBuffer;

type UartRegs = hal::pac::USART1;
type UartRegisterBlock = hal::pac::usart1::RegisterBlock;
type TxPin = gpio::gpioa::PA9<gpio::Alternate<gpio::AF1>>;
type RxPin = gpio::gpioa::PA10<gpio::Alternate<gpio::AF1>>;
type TxDma = super::dma::DmaChannel<2>;
type RxDma = super::dma::DmaChannel<3>;

/// DMA UART
pub struct Uart<RXBUF> {
    pub tx: Tx,
    pub rx: Rx<RXBUF>,
}

/// DMA UART TX half
///
/// Transmits data over DMA. Use `transmit()` to copy data to the DMA buffer.
/// After transfer completion DMA transfer complete interrupt will fire and
/// the `finish()` *must* be called in the interrupt service routine.
///
/// Make sure to never send more than data than the buffer size of the RX half
/// without an Idle line in between.
// TODO: provide way to ensure idle line or update the receiver to also grab
// the data on half/full transfer complete interrupts.
pub struct Tx {
    dma: TxDma,
    buf: &'static mut [u8],
    ready: bool,
}

/// DMA UART RX half
///
/// UART receiver using DMA with a circular buffer. DMA is configured to transfer
/// BUF.len() data in circular mode. Receiver uses 2 interrupts:
///
/// * UART Idle Line interrupt: used to detect when data transmission stops.
///   `on_uart_interrupt` method should be called in the UART interrupt routine.
/// * DMA transfer complete interrupt: call `on_transfer_complete` in the interrupt
///   service routine - it is needed to correctly retrieve data after DMA wraps
///   around the buffer.
pub struct Rx<BUF> {
    dma: RxDma,
    buf: CircularBuffer<BUF>,
}

/// Valid data from RX buffer
///
/// TODO: safety - it is possible that DMA will overwrite data if we are not
/// able to copy it fast enough
pub struct RxData<'a> {
    data1: &'a [u8],  // main slice
    data2: &'a [u8],  // 2nd slice with data after buffer wrap
    overwritten: usize,
}

impl<BUF> Uart<BUF>
where
    BUF: StaticWriteBuffer<Word = u8>
{
    // TODO: use builder pattern or a config struct
    pub fn new(
        uart: UartRegs,
        (tx, rx): (TxPin, RxPin),
        (tx_dma, rx_dma): (TxDma, RxDma),
        (tx_buf, rx_buf): (&'static mut [u8], BUF),
        baud_rate: hal::time::Bps,
        rcc: &mut hal::rcc::Rcc,
    ) -> Self
    {
        // Need to access `.regs` but it's private
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        // Enable UART clock and reset peripheral
        rcc_regs.apb2enr.modify(|_, w| w.usart1en().enabled());
        rcc_regs.apb2rstr.modify(|_, w| w.usart1rst().set_bit());
        rcc_regs.apb2rstr.modify(|_, w| w.usart1rst().clear_bit());

        // Calculate baudrate divisor
        let brr = rcc.clocks.pclk().0 / baud_rate.0;
        uart.brr.write(|w| unsafe { w.bits(brr) });

        // Common UART configuration - mostly defaults (CR1/2/3 reset via APB2RSTR)
        // TX/RX-specific configuration in respective constructors
        uart.cr1.write(|w| w.ue().enabled());

        Self {
            tx: Tx::new(tx, tx_dma, tx_buf),
            rx: Rx::new(rx, rx_dma, rx_buf),
        }
    }

    pub fn split(self) -> (Tx, Rx<BUF>) {
        (self.tx, self.rx)
    }
}

impl Tx {
    fn new(_pin: TxPin, mut dma: TxDma, buf: &'static mut [u8]) -> Self {
        // Configure DMA
        dma.ch().cr.write(|w| {
            w
                .dir().from_memory()
                .mem2mem().disabled()
                .circ().disabled()
                .minc().enabled()
                .pinc().disabled()
                .msize().bits8()
                .psize().bits8()
                .pl().low() // TODO: configurable priority
                .htie().disabled()
                .tcie().enabled()
                .teie().enabled()
        });


        // Enable UART. This will send an Idle Frame as first transmission, but
        // we no need to wait as we check transfer complete in transmit() anyway.
        Self::uart().cr1.modify(|_, w| w.te().enabled());

        Self { dma, buf, ready: true }
    }

    pub fn transmit(&mut self, data: &[u8]) -> nb::Result<(), Infallible> {
        // Check TC bit to wait for transmission complete, and TEACK bit to
        // check if TE=1 after IDLE line from finish(). This will never be 1
        // if for some reason TE has been set to 0 witout re-setting to 1.
        let isr = Self::uart().isr.read();
        let uart_ready = isr.tc().bit_is_clear() || isr.teack().bit_is_clear();
        // TODO: In SPI we only return on !ready, but spin lock on SPI FIFO,
        // should we do the same here or change SPI implementation?
        if !self.ready || uart_ready {
            return Err(nb::Error::WouldBlock);
        }

        self.buf[..data.len()].copy_from_slice(data);
        self.configure_dma_transfer(data.len());

        atomic::compiler_fence(atomic::Ordering::Release);

        // Enable DMA channel and trigger DMA TX request
        self.dma.ch().cr.modify(|_, w| w.en().enabled());
        Self::uart().cr3.modify(|_, w| w.dmat().enabled());

        Ok(())
    }

    pub fn finish(&mut self) -> Result<bool, ()> {
        let isr = self.dma.isr();
        if !isr.any() {
            // not an interrupt from our channel
            return Ok(false);
        }

        // Clear flags
        self.dma.ifcr(|w| w.all());

        // Disable DMA request and channel
        Self::uart().cr3.modify(|_, w| w.dmat().disabled());
        self.dma.ch().cr.modify(|_, w| w.en().disabled());

        if isr.error() {
            // TODO: error handling
            return Err(())
        }

        // Ensure idle frame after transfer
        // FIXME: sometimes waiting for TEACK leads to an infinite loop
        // Self::uart().cr1.modify(|_, w| w.te().disabled());
        // // We must check TEACK to ensure that TE=0 has been registered.
        // while Self::uart().isr.read().teack().bit_is_clear() {}
        // Self::uart().cr1.modify(|_, w| w.te().enabled());
        // // Do not wait for TEACK=1, we will wait in transmit() if needed.

        atomic::compiler_fence(atomic::Ordering::Acquire);

        self.ready = true;
        Ok(true)
    }

    fn configure_dma_transfer(&mut self, len: usize) {
        let src = self.buf.as_ptr();
        let dst = Self::uart().tdr.as_ptr() as u32;
        self.dma.ch().mar.write(|w| unsafe { w.ma().bits(src as u32) });
        self.dma.ch().par.write(|w| unsafe { w.pa().bits(dst) });
        self.dma.ch().ndtr.write(|w| w.ndt().bits(len as u16));
    }

    fn uart() -> &'static UartRegisterBlock {
        unsafe { &*UartRegs::ptr() }
    }
}

impl<BUF> Rx<BUF>
where
    BUF: StaticWriteBuffer<Word = u8>
{
    fn new(_pin: RxPin, mut dma: RxDma, mut buf: BUF) -> Self {
        let uart = unsafe { &*UartRegs::ptr() };

        // Configure UART RX half
        // TODO: or use receiver timeout interrupt?
        uart.cr1.modify(|_, w| w.idleie().enabled().re().enabled());
        uart.cr3.modify(|_, w| w.dmar().enabled());

        // Configure DMA
        dma.ch().cr.write(|w| {
            w
                .dir().from_peripheral()
                .mem2mem().disabled()
                .circ().enabled()
                .minc().enabled()
                .pinc().disabled()
                .msize().bits8()
                .psize().bits8()
                .pl().medium()
                .htie().disabled()
                .tcie().enabled()
                .teie().enabled()
        });

        // Configure DMA data transfers
        let src = uart.rdr.as_ptr() as u32;
        let (dst, len) = unsafe { buf.write_buffer() };
        dma.ch().par.write(|w| unsafe { w.pa().bits(src) });
        dma.ch().mar.write(|w| unsafe { w.ma().bits(dst as u32) });
        dma.ch().ndtr.write(|w| w.ndt().bits(len as u16));

        // Start reception
        atomic::compiler_fence(atomic::Ordering::Release);
        dma.ch().cr.modify(|_, w| w.en().enabled());

        Self { dma, buf: CircularBuffer::new(buf) }
    }

    pub fn consume(&mut self) -> RxData {
        let buf_len = unsafe { self.buf.write_buffer().1 as u16 };
        let remaining = self.dma.ch().ndtr.read().ndt().bits();
        // Tail is where DMA is currently writing
        let tail = buf_len - remaining;

        atomic::compiler_fence(atomic::Ordering::Acquire);

        let (data1, data2, overwritten) = self.buf.consume(tail);
        RxData { data1, data2, overwritten }
    }

    pub fn on_uart_interrupt(&mut self) -> Option<RxData> {
        let uart = unsafe { &*UartRegs::ptr() };
        if uart.isr.read().idle().bit_is_set() {
            uart.icr.write(|w| w.idlecf().clear());
            Some(self.consume())
        } else {
            None
        }
    }

    pub fn on_transfer_complete(&mut self) -> Result<bool, ()> {
        let isr = self.dma.isr();
        if !isr.any() {
            // not an interrupt from our channel
            return Ok(false);
        }

        // Clear interrupt flags
        self.dma.ifcr(|w| w.all());

        if isr.complete() {
            self.buf.tail_wrapped();
        }

        if isr.error() {
            Err(())
        } else {
            Ok(true)
        }
    }
}

impl<'a> RxData<'a> {
    pub fn data(&self) -> (&'a [u8], &'a [u8]) {
        (self.data1, self.data2)
    }

    pub fn lost(&self) -> usize {
        self.overwritten
    }

    pub fn safety_margin(&self) -> usize {
        todo!()
    }

    pub fn iter_all(&self) -> impl Iterator<Item = &'a u8> {
        self.data1.iter().chain(self.data2.iter())
    }

    pub fn len(&self) -> usize {
        self.data1.len() + self.data2.len()
    }
}

impl<'a> core::ops::Index<usize> for RxData<'a> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        if index < self.data1.len() {
            &self.data1[index]
        } else {
            &self.data2[index - self.data1.len()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rx_data_index() {
        let (buf1, buf2) = ([1, 2, 3, 4, 5], [6, 7, 8, 9]);
        let rx_data = RxData { data1: &buf1, data2: &buf2, overwritten: 0 };
        assert_eq!(rx_data[0], 1);
        assert_eq!(rx_data[3], 4);
        assert_eq!(rx_data[4], 5);
        assert_eq!(rx_data[5], 6);
        assert_eq!(rx_data[6], 7);
        assert_eq!(rx_data[8], 9);
    }
}
