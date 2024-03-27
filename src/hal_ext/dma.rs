use crate::hal;

/// Extension trait to split DMA into separate channels
pub trait DmaSplit {
    /// Structure holding DMA channles
    type Channels;

    /// Split DMA into independent channels
    fn split(self, rcc: &mut hal::rcc::Rcc) -> Self::Channels;
}

/// Single DMA channel
pub struct DmaChannel<const C: u8>;

/// ISR flags for a single DMA channel
#[derive(PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub struct InterruptStatus(u8);

/// IFCR flags for a single DMA channel
#[derive(PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub struct InterruptClear(u8);

/// DMA Interrupt type to handle (error is always checked)
pub enum Interrupt {
    FullTransfer,
    HalfTransfer,
}

/// Result of handling DMA interrupt
#[derive(PartialEq, Eq)]
pub enum InterruptResult {
    /// Interrupt flag wasn't set, nothing has been done
    NotSet,
    /// Interrupt handled and the flag has been cleared
    Done,
    /// Interrupt error flag was set; flags have been cleared and interrupts disabled
    Error,
}

/// All DMA channels on the MCU
pub struct Dma {
    pub ch1: DmaChannel<1>,
    pub ch2: DmaChannel<2>,
    pub ch3: DmaChannel<3>,
    pub ch4: DmaChannel<4>,
    pub ch5: DmaChannel<5>,
    pub ch6: DmaChannel<6>,
    pub ch7: DmaChannel<7>,
}

/// DMA transfer ongoing error
#[cfg_attr(test, derive(Debug))]
pub struct TransferOngoing;

/// Trait representing buffered DMA transmitter
pub trait DmaTx {
    /// Get per-transfer capacity of the DMA buffer
    fn capacity(&self) -> usize;

    /// Check if DMA transfer is ready/ongoing
    fn is_ready(&self) -> bool;

    /// Push data to the internal buffer
    ///
    /// This interface allows to copy data to internal buffer which is passed
    /// as an argument to `writer` callback, which should return the number of
    /// data written.
    fn push<F: FnOnce(&mut [u8]) -> usize>(&mut self, writer: F) -> Result<(), TransferOngoing>;

    /// Start transmiting data
    ///
    /// If the previous transfer is not complete returns [`TransferOngoing`], if it is
    /// complete but there is minimal waiting required (e.g. for a TX FIFO flag) then
    /// returns [`nb::Error::WouldBlock`].
    fn start(&mut self) -> nb::Result<(), TransferOngoing>;

    /// Handle DMA TX complete interrupt
    ///
    /// The return value has the same meaning as in [`DmaChannel::handle_interrupt`].
    fn on_interrupt(&mut self) -> InterruptResult;

    /// Transmit data (shorthand for copy to `buf_mut()` followed by `start()`)
    // Note:
    // Initially tried the following interface:
    //   fn transmit<I: IntoIterator<Item = u8>>(&mut self, data: I) -> nb::Result<usize, TransferOngoing>;
    // but this is very likely less efficient (likely no memcpy) and there is a problem
    // when other components want to serialize to a slice in their interface.
    fn transmit(&mut self, data: &[u8]) -> nb::Result<(), TransferOngoing> {
        self.push(|buf| {
            buf[..data.len()].copy_from_slice(data);
            data.len()
        }).map_err(nb::Error::Other)?;
        self.start()
    }
}

/// Trait representing DMA receiver
pub trait DmaRx {
    /// Read the received data (if any)
    fn read<F: FnMut(&[u8])>(&mut self, reader: F);

    /// Remaining capacity of the internal buffer
    fn capacity_remaining(&mut self) -> usize;

    /// Handle interrupt and read out received data
    fn on_interrupt<F: FnMut(&[u8])>(&mut self, reader: F) -> InterruptResult;
}

impl DmaSplit for hal::pac::DMA1 {
    type Channels = Dma;

    fn split(self, _rcc: &mut hal::rcc::Rcc) -> Self::Channels {
        // Need to access some registers outside of HAL type system (field `regs` is private)
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        // Enable DMA clock
        rcc_regs.ahbenr.modify(|_, w| w.dmaen().enabled());

        Dma {
            // isr: todo!(),
            // ifcr: todo!(),
            ch1: DmaChannel,
            ch2: DmaChannel,
            ch3: DmaChannel,
            ch4: DmaChannel,
            ch5: DmaChannel,
            ch6: DmaChannel,
            ch7: DmaChannel,
        }
    }
}

macro_rules! dma_channels {
    ($($C:literal => $ch:ident),+ $(,)?) => {
        $(
            impl DmaChannel<$C> {
                /// Access DMA register associated with this channel
                // Safety: takes &mut, so it's not possible to use channel in multiple places?
                pub fn ch(&mut self) -> &hal::pac::dma1::CH {
                    unsafe { &(*hal::pac::DMA1::ptr()).$ch }
                }

                const OFFSET: usize = 4 * ($C - 1);
                const MASK: u32 = 0b1111;

                /// Read interrupt status flags for this channel
                pub fn isr(&self) -> InterruptStatus {
                    let dma = unsafe { &*hal::pac::DMA1::ptr() };
                    let isr = dma.isr.read().bits();
                    let masked = ((isr >> Self::OFFSET) & Self::MASK) as u8;
                    InterruptStatus(masked)
                }

                /// Clear interrupt flags for this channel
                pub fn ifcr<F>(&mut self, f: F)
                where
                    F: FnOnce(&mut InterruptClear) -> &mut InterruptClear
                {
                    let dma = unsafe { &*hal::pac::DMA1::ptr() };
                    let mut ifcr = InterruptClear(0);
                    let ifcr = f(&mut ifcr);
                    let mask = (ifcr.0 as u32 & Self::MASK) << Self::OFFSET;
                    unsafe { dma.ifcr.write(|w| w.bits(mask)); }
                }

                /// Handle transfer completion (or error) interrupt if it occured
                pub fn handle_interrupt(&mut self, interrupt: Interrupt) -> InterruptResult {
                    // Check if this is an interrupt from this channel (only the one we care about!)
                    let isr = self.isr();
                    let is_set = match interrupt {
                        Interrupt::FullTransfer => isr.complete(),
                        Interrupt::HalfTransfer => isr.half_complete(),
                    };
                    if !(is_set || isr.error()) {
                        return InterruptResult::NotSet;
                    }

                    // Clear only the flags we checked! This is important because new flags
                    // could have been set since the moment we read the status register.
                    self.ifcr(|w| {
                        match interrupt {
                            Interrupt::FullTransfer => w.complete(),
                            Interrupt::HalfTransfer => w.half_complete(),
                        }.error()
                    });

                    if isr.error() {
                        // On error hardware clears EN bit, we disable all interrupts
                        self.ch().cr.modify(|_, w| {
                            w
                                .htie().disabled()
                                .tcie().disabled()
                                .teie().disabled()
                        });
                        InterruptResult::Error
                    } else {
                        InterruptResult::Done
                    }
                }
            }
        )+
    }
}

dma_channels!(
    1 => ch1,
    2 => ch2,
    3 => ch3,
    4 => ch4,
    5 => ch5,
    6 => ch6,
    7 => ch7,
);

impl InterruptStatus {
    /// GIFx flag
    pub fn any(&self) -> bool {
        (self.0 & 0b0001) != 0
    }

    /// TCIFx flag
    pub fn complete(&self) -> bool {
        (self.0 & 0b0010) != 0
    }

    /// HTIFx flag
    pub fn half_complete(&self) -> bool {
        (self.0 & 0b0100) != 0
    }

    /// TEIFx flag
    ///
    /// A DMA error is generated when redaing from or writing to a reserved address space.
    pub fn error(&self) -> bool {
        (self.0 & 0b1000) != 0
    }

    /// Replace error flag with [`Err`]
    ///
    /// A DMA error is generated when reading from or writing to a reserved address space.
    #[allow(clippy::result_unit_err)]
    pub fn as_result(self) -> Result<Self, ()> {
        if self.error() {
            Err(())
        } else {
            let mut status = self.0 & 0b0110;
            if status != 0 {
                status |= 0b001;
            }
            Ok(Self(status))
        }
    }
}

impl InterruptClear {
    /// GIFx flag
    pub fn all(&mut self) -> &mut Self {
        self.0 |= 0b0001;
        self
    }

    /// TCIFx flag
    pub fn complete(&mut self) -> &mut Self {
        self.0 |= 0b0010;
        self
    }

    /// HTIFx flag
    pub fn half_complete(&mut self) -> &mut Self {
        self.0 |= 0b0100;
        self
    }

    /// TEIFx flag
    pub fn error(&mut self) -> &mut Self {
        self.0 |= 0b1000;
        self
    }
}

impl InterruptResult {
    /// Ignore case when no interrupt flag was set
    ///
    /// This is useful when we only want to check for potential errors.
    pub fn if_any(&self) -> &Result<(), ()> {
        match self {
            Self::NotSet | Self::Done => &Ok(()),
            Self::Error => &Err(()),
        }
    }

    /// Transform to an option (if set) containing result (done/error)
    pub fn as_option(&self) -> &Option<Result<(), ()>> {
        match self {
            Self::NotSet => &None,
            Self::Done => &Some(Ok(())),
            Self::Error => &Some(Err(())),
        }
    }
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::vec::Vec;

    pub struct DmaTxMock<C: FnMut(Vec<u8>), const N: usize> {
        buf: [u8; N],
        len: usize,
        ready: bool,
        instant_interrupt: bool,
        callback: C,
    }

    impl<C: FnMut(Vec<u8>), const N: usize> DmaTxMock<C, N> {
        pub fn new(instant_interrupt: bool, callback: C) -> Self {
            Self { buf: [0; N], len: 0, ready: true, instant_interrupt, callback }
        }
    }

    impl<C: FnMut(Vec<u8>), const N: usize> DmaTx for DmaTxMock<C, N> {
        fn capacity(&self) -> usize {
            N
        }

        fn is_ready(&self) -> bool {
            self.ready
        }

        fn push<F: FnOnce(&mut [u8]) -> usize>(&mut self, writer: F) -> Result<(), TransferOngoing> {
            if !self.is_ready() {
                return Err(TransferOngoing);
            }
            self.len = writer(&mut self.buf);
            Ok(())
        }

        fn start(&mut self) -> nb::Result<(), TransferOngoing> {
            if !self.is_ready() {
                return Err(nb::Error::Other(TransferOngoing));
            }
            if self.len != 0 {
                self.ready = false;
            }
            if self.instant_interrupt {
                self.on_interrupt();
            }
            Ok(())
        }

        fn on_interrupt(&mut self) -> InterruptResult {
            if !self.ready {
                (self.callback)(self.buf[..self.len].into());
                self.len = 0;
                self.ready = true;
                InterruptResult::Done
            } else {
                InterruptResult::NotSet
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_register_offsets() {
        // offset to ISR.GIFx bit
        assert_eq!(DmaChannel::<1>::OFFSET, 0);
        assert_eq!(DmaChannel::<2>::OFFSET, 4);
        assert_eq!(DmaChannel::<7>::OFFSET, 24);
    }

    #[test]
    fn channel_register_mask() {
        assert_eq!(DmaChannel::<1>::MASK << DmaChannel::<1>::OFFSET, 0b0000_0000_0000_0000_0000_0000_0000_1111);
        assert_eq!(DmaChannel::<2>::MASK << DmaChannel::<2>::OFFSET, 0b0000_0000_0000_0000_0000_0000_1111_0000);
        assert_eq!(DmaChannel::<7>::MASK << DmaChannel::<7>::OFFSET, 0b0000_1111_0000_0000_0000_0000_0000_0000);
    }

    #[test]
    fn interrupt_status() {
        assert_eq!(InterruptStatus(0b0000).any(), false);
        assert_eq!(InterruptStatus(0b0000).half_complete(), false);
        assert_eq!(InterruptStatus(0b0001).any(), true);
        assert_eq!(InterruptStatus(0b0001).half_complete(), false);
        assert_eq!(InterruptStatus(0b0100).any(), false);
        assert_eq!(InterruptStatus(0b0100).half_complete(), true);
    }

    #[test]
    fn interrupt_clear() {
        assert_eq!(InterruptClear(0).0, 0b0000);
        assert_eq!(InterruptClear(0).complete().half_complete().0, 0b0110);
        assert_eq!(InterruptClear(0).error().all().0, 0b1001);
    }

    #[test]
    fn interrupt_status_as_result() {
        assert_eq!(InterruptStatus(0b1000).as_result(), Err(()));
        assert_eq!(InterruptStatus(0b0010).as_result().unwrap().complete(), true);
        assert_eq!(InterruptStatus(0b0010).as_result().unwrap().any(), true);
        assert_eq!(InterruptStatus(0b0100).as_result().unwrap().half_complete(), true);
        assert_eq!(InterruptStatus(0b0100).as_result().unwrap().any(), true);
        assert_eq!(InterruptStatus(0b0000).as_result().unwrap().any(), false);
    }
}
