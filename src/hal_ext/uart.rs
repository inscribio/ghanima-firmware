use core::convert::Infallible;
use core::sync::atomic;
use embedded_dma::WriteBuffer;
use bbqueue::{Producer, Consumer, BBBuffer, GrantR};

use crate::utils::InfallibleResult;
use crate::hal;
use hal::gpio;
use super::circ_buf::CircularBuffer;
use super::dma;

type UartRegs = hal::pac::USART1;
type UartRegisterBlock = hal::pac::usart1::RegisterBlock;
type TxPin = gpio::gpioa::PA9<gpio::Alternate<gpio::AF1>>;
type RxPin = gpio::gpioa::PA10<gpio::Alternate<gpio::AF1>>;
type TxDma = dma::DmaChannel<2>;
type RxDma = dma::DmaChannel<3>;

/// DMA UART
pub struct Uart<const TX: usize, const RX: usize, RXBUF> {
    /// UART TX half
    pub tx: Tx<TX>,
    pub tx_queue: Producer<'static, TX>,
    /// UART RX half
    pub rx: Rx<RX, RXBUF>,
    pub rx_queue: Consumer<'static, RX>
}

/// DMA UART TX half
///
/// Transmits data over DMA providing the [`dma::DmaTx`] interface. Designed
/// to work well with [`Rx`]. Make sure to never send more than data than
/// the buffer size of the RX half without an Idle line in between to avoid
/// buffer overruns.
pub struct Tx<const N: usize> {
    dma: TxDma,
    consumer: Consumer<'static, N>,
    transfer: Option<GrantR<'static, N>>,
}

/// DMA UART RX half
///
/// UART receiver with DMA. DMA is configured to transfer to BUF in circular mode,
/// because (from tests) there is too much delay when trying to disable DMA, then
/// enable it to a different memory region and we are hitting UART overrun errors.
/// In circular mode DMA is writing to BUF which is a "smaller buffer" - its size
/// just limits the number of IRQs that will fire during a single transfer. In each
/// interrupt data is copied from BUF to the producer (main queue - "larger buffer").
///
/// UART reception uses Idle Line interrupt so for a single message the reception
/// latency is relatively low. For continuous stream of data the latency depends on
/// BUF size and is up to half of its size.
///
/// User must call [`Rx::on_uart_interrupt`] in UART IRQ handler and
/// [`Rx::on_dma_interrupt`] in DMA IRQ handler. These functions will handle clearing
/// IRQ flags.
pub struct Rx<const N: usize, BUF> {
    dma: RxDma,
    producer: Producer<'static, N>,
    buf: CircularBuffer<BUF>,
}

#[allow(dead_code)]
struct ConsumeResult {
    written: u16,
    lost: u16,
}

impl<const TX: usize, const RX: usize, RXBUF> Uart<TX, RX, RXBUF>
where
    RXBUF: WriteBuffer<Word = u8>
{
    /// Configure DMA UART with given baud rate
    // TODO: use builder pattern or a config struct
    pub fn new(
        uart: UartRegs,
        (tx, rx): (TxPin, RxPin),
        (tx_dma, rx_dma): (TxDma, RxDma),
        (tx_buf, rx_bbbuf, rx_buf): (&'static BBBuffer<TX>, &'static BBBuffer<RX>, RXBUF),
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

        let (tx, tx_queue) = Tx::new(tx, tx_dma, tx_buf);
        let (rx, rx_queue) = Rx::new(rx, rx_dma, rx_bbbuf, rx_buf);
        Self { tx, tx_queue, rx, rx_queue }
    }

    /// Split UART into separate TX/RX halves
    pub fn split(self) -> (Tx<TX>, Producer<'static, TX>, Rx<RX, RXBUF>, Consumer<'static, RX>) {
        (self.tx, self.tx_queue, self.rx, self.rx_queue)
    }
}

impl<const N: usize> Tx<N> {
    fn new(_pin: TxPin, mut dma: TxDma, buf: &'static BBBuffer<N>) -> (Self, Producer<'static, N>) {
        let (producer, consumer) = buf.try_split().unwrap();

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
                .pl().low()
                .htie().disabled()
                .tcie().enabled()
                .teie().enabled()
        });


        // Enable UART. This will send an Idle Frame as first transmission, but
        // we no need to wait as we check transfer complete in transmit() anyway.
        Self::uart().cr1.modify(|_, w| w.te().enabled());

        (Self { dma, consumer, transfer: None }, producer)
    }

    fn configure_dma_transfer(&mut self, buf: &'static [u8]) {
        let src = buf.as_ptr();
        let dst = Self::uart().tdr.as_ptr() as u32;
        self.dma.ch().mar.write(|w| unsafe { w.ma().bits(src as u32) });
        self.dma.ch().par.write(|w| unsafe { w.pa().bits(dst) });
        self.dma.ch().ndtr.write(|w| w.ndt().bits(buf.len() as u16));
    }

    fn uart() -> &'static UartRegisterBlock {
        unsafe { &*UartRegs::ptr() }
    }

    fn start_dma(&mut self) -> nb::Result<(), Infallible> {
        // Check TC bit to wait for transmission complete, and TEACK bit to
        // check if TE=1 after IDLE line from finish(). This will never be 1
        // if for some reason TE has been set to 0 without re-setting to 1.
        let isr = Self::uart().isr.read();
        if !(isr.tc().bit_is_set() && isr.teack().bit_is_set()) {
            return Err(nb::Error::WouldBlock);
        }

        atomic::compiler_fence(atomic::Ordering::Release);

        // Enable DMA channel and trigger DMA TX request
        self.dma.ch().cr.modify(|_, w| w.en().enabled());
        Self::uart().cr3.modify(|_, w| w.dmat().enabled());

        Ok(())
    }

    fn stop_dma(&mut self) {
        // Disable DMA request and channel
        Self::uart().cr3.modify(|_, w| w.dmat().disabled());
        self.dma.ch().cr.modify(|_, w| w.en().disabled());

        atomic::compiler_fence(atomic::Ordering::Acquire);
    }

    /// Start next transfer if there is data available. This may block until UART transmission
    /// complete flag is set.
    pub fn tick(&mut self) -> bool {
        if self.transfer.is_some() {
            return false;
        }

        let grant = match self.consumer.read() {
            Ok(grant) => grant,
            Err(e) => match e {
                bbqueue::Error::InsufficientSize => return false,
                bbqueue::Error::GrantInProgress => unreachable!(),
                bbqueue::Error::AlreadySplit => unreachable!(),
            }
        };

        // Safety: we're not releasing the grant until DMA finishes
        self.configure_dma_transfer(unsafe { grant.as_static_buf() });
        self.transfer = Some(grant);
        nb::block!(self.start_dma()).infallible();

        true
    }

    pub fn on_dma_interrupt(&mut self) -> dma::InterruptResult {
        let res = self.dma.handle_interrupt(dma::Interrupt::FullTransfer);
        if let Some(status) = res.as_option() {
            self.stop_dma();

            if status.is_ok() {
                if let Some(grant) = self.transfer.take() {
                    let len = grant.len();
                    grant.release(len);
                } else {
                    unreachable!("Transfer completion but transfer have not been started");
                }

                self.tick();
            }
        }
        res
    }
}

// impl dma::DmaTx for Tx {
//     fn capacity(&self) -> usize {
//         self.buf.len()
//     }
//
//     fn is_ready(&self) -> bool {
//         self.ready
//     }
//
//     fn push<F: FnOnce(&mut [u8]) -> usize>(&mut self, writer: F) -> Result<(), dma::TransferOngoing> {
//         if !self.is_ready() {
//             return Err(dma::TransferOngoing);
//         }
//         let len = writer(self.buf);
//         self.configure_dma_transfer(len);
//         Ok(())
//     }
//
//     fn start(&mut self) -> nb::Result<(), dma::TransferOngoing> {
//         if !self.is_ready() {
//             return Err(nb::Error::Other(dma::TransferOngoing));
//         }
//
//         // Check TC bit to wait for transmission complete, and TEACK bit to
//         // check if TE=1 after IDLE line from finish(). This will never be 1
//         // if for some reason TE has been set to 0 witout re-setting to 1.
//         let isr = Self::uart().isr.read();
//         if !(isr.tc().bit_is_set() && isr.teack().bit_is_set()) {
//             return Err(nb::Error::WouldBlock);
//         }
//
//         if self.len() == 0 {
//             return Ok(());
//         }
//
//         self.ready = false;
//
//         atomic::compiler_fence(atomic::Ordering::Release);
//
//         // Enable DMA channel and trigger DMA TX request
//         self.dma.ch().cr.modify(|_, w| w.en().enabled());
//         Self::uart().cr3.modify(|_, w| w.dmat().enabled());
//
//         Ok(())
//     }
//
//     fn on_interrupt(&mut self) -> dma::InterruptResult {
//         let res = self.dma.handle_interrupt(dma::Interrupt::FullTransfer);
//         if let Some(status) = res.as_option() {
//             // Disable DMA request and channel
//             Self::uart().cr3.modify(|_, w| w.dmat().disabled());
//             self.dma.ch().cr.modify(|_, w| w.en().disabled());
//
//             atomic::compiler_fence(atomic::Ordering::Acquire);
//
//             if status.is_ok() {
//                 assert!(!self.ready, "Transfer completion but transfer have not been started");
//                 self.ready = true;
//
//                 // Ensure idle frame after transfer
//                 // FIXME: sometimes waiting for TEACK leads to an infinite loop
//                 // Self::uart().cr1.modify(|_, w| w.te().disabled());
//                 // // We must check TEACK to ensure that TE=0 has been registered.
//                 // while Self::uart().isr.read().teack().bit_is_clear() {}
//                 // Self::uart().cr1.modify(|_, w| w.te().enabled());
//                 // // Do not wait for TEACK=1, we will wait in transmit() if needed.
//             }
//         }
//         res
//     }
// }

impl<const N: usize, BUF> Rx<N, BUF>
where
    BUF: WriteBuffer<Word = u8>
{
    pub fn new(_pin: RxPin, mut dma: RxDma, bbbuf: &'static BBBuffer<N>, buf: BUF) -> (Self, Consumer<'static, N>) {
        let (producer, consumer) = bbbuf.try_split().unwrap();

        let uart = Self::uart();

        // Enable UART RX half with idle interrupt
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
                .htie().enabled()
                .tcie().enabled()
                .teie().enabled()
        });

        let mut buf = CircularBuffer::new(buf);

        // Configure circular DMA data transfers to the intermediate buffer
        let src = uart.rdr.as_ptr() as u32;
        let (dst, len) = unsafe { buf.write_buffer() };
        dma.ch().par.write(|w| unsafe { w.pa().bits(src) });
        dma.ch().mar.write(|w| unsafe { w.ma().bits(dst as u32) });
        dma.ch().ndtr.write(|w| w.ndt().bits(len as u16));

        // Start reception
        atomic::compiler_fence(atomic::Ordering::Release);
        dma.ch().cr.modify(|_, w| w.en().enabled());

        let rx = Self { dma, producer, buf };
        (rx, consumer)
    }

    fn uart() -> &'static UartRegisterBlock {
        unsafe { &*UartRegs::ptr() }
    }

    fn tail(&mut self) -> u16 {
        let buf_len = unsafe { self.buf.write_buffer().1 as u16 };
        let remaining = self.dma.ch().ndtr.read().ndt().bits();
        // Tail is where DMA is currently writing
        buf_len - remaining
    }

    // Copy as much as possible from src to dst
    fn extend_from_slice<'a, 'b>(dst: &'a mut [u8], src: &'b [u8]) -> (&'a mut [u8], &'b [u8]) {
        let n = dst.len().min(src.len());
        dst[..n].copy_from_slice(&src[..n]);
        (&mut dst[n..], &src[n..])
    }

    // Returns total number of copied data
    fn consume_step<'a>(prod: &mut Producer<'static, N>, mut data1: &'a [u8], mut data2: &'a [u8]) -> (usize, &'a [u8], &'a [u8]) {
        let len = data1.len() + data2.len();
        let mut grant = match prod.grant_max_remaining(len) {
            Ok(grant) => grant,
            Err(e) => match e {
                bbqueue::Error::InsufficientSize => return (0, data1, data2),
                bbqueue::Error::GrantInProgress => unreachable!(),
                bbqueue::Error::AlreadySplit => unreachable!(),
            },
        };

        let mut window = grant.buf();
        (window, data1) = Self::extend_from_slice(window, data1);
        (window, data2) = Self::extend_from_slice(window, data2);
        let remaining = window.len();

        let copied = grant.len() - remaining;
        grant.commit(copied);

        (copied, data1, data2)
    }

    // Regardless of success returns amount of data that has been lost
    fn consume(&mut self) -> ConsumeResult {
        let tail = self.tail();

        atomic::compiler_fence(atomic::Ordering::Acquire);

        let (mut data1, mut data2, overwritten) = self.buf.consume(tail);
        let total_len = data1.len() + data2.len();

        // First iteration - try to fill the buffer until the end
        let mut copied;
        (copied, data1, data2) = Self::consume_step(&mut self.producer, data1, data2);

        // Second iteration if the buffer should wrap (i.e. granted < requested)
        if copied < total_len {
            let copied2;
            (copied2, _, _) = Self::consume_step(&mut self.producer, data1, data2);
            copied += copied2;
        }

        ConsumeResult {
            written: copied as u16,
            lost: (overwritten + (total_len - copied)) as u16,
        }
    }

    /// Handle UART interrupt
    pub fn on_uart_interrupt(&mut self) -> dma::InterruptResult { // TODO: custom return type?
        let uart = Self::uart();
        if uart.isr.read().idle().bit_is_set() {
            uart.icr.write(|w| w.idlecf().clear());
            self.consume();
            dma::InterruptResult::Done
        } else {
            dma::InterruptResult::NotSet
        }
    }

    /// Handle DMA interrupt
    pub fn on_dma_interrupt(&mut self) -> dma::InterruptResult {
        let half = self.dma.handle_interrupt(dma::Interrupt::HalfTransfer);
        let full = self.dma.handle_interrupt(dma::Interrupt::FullTransfer);
        if full == dma::InterruptResult::Done {
            self.buf.tail_wrapped();
        }
        if half == dma::InterruptResult::Done || full == dma::InterruptResult::Done {
            self.consume();
        }
        match (half, full) {
            (dma::InterruptResult::Error, _) => dma::InterruptResult::Error,
            (_, dma::InterruptResult::Error) => dma::InterruptResult::Error,
            (dma::InterruptResult::NotSet, r) => r,
            (r, dma::InterruptResult::NotSet) => r,
            (dma::InterruptResult::Done, dma::InterruptResult::Done) => dma::InterruptResult::Done,
        }
    }
}
