use crate::hal;

/// Extension trait to split DMA into separate channels
pub trait DmaSplit {
    /// Structure holding DMA channles
    type Channels;

    /// Split DMA into independent channels
    fn split(self, rcc: &mut hal::rcc::Rcc) -> Self::Channels;
}

pub struct DmaChannel<const C: u8>;

/// ISR flags for a single DMA channel
#[derive(Debug, PartialEq, Eq)]
pub struct InterruptStatus(u8);

/// IFCR flags for a single DMA channel
#[derive(Debug, PartialEq, Eq)]
pub struct InterruptClear(u8);

pub struct Dma {
    pub ch1: DmaChannel<1>,
    pub ch2: DmaChannel<2>,
    pub ch3: DmaChannel<3>,
    pub ch4: DmaChannel<4>,
    pub ch5: DmaChannel<5>,
    pub ch6: DmaChannel<6>,
    pub ch7: DmaChannel<7>,
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
                // Safety: takes &mut, so it's not possible to use channel in multiple places?
                pub fn ch(&mut self) -> &hal::pac::dma1::CH {
                    unsafe { &(*hal::pac::DMA1::ptr()).$ch }
                }

                const OFFSET: usize = 4 * ($C - 1);
                const MASK: u32 = 0b1111;

                pub fn isr(&self) -> InterruptStatus {
                    let dma = unsafe { &*hal::pac::DMA1::ptr() };
                    InterruptStatus(((dma.isr.read().bits() >> Self::OFFSET) & Self::MASK) as u8)
                }

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

                /// Clear channel interrupt flags and check which interrupts occured
                ///
                /// If there was an error flag it will be returned as `Err`, and the
                /// half/full-transfer interrupts can be found in `Ok` vairant.
                pub fn handle_interrupt(&mut self) -> Result<InterruptStatus, ()> {
                    // Check if this is an interrupt from this channel
                    let isr = self.isr();
                    if !isr.any() {
                        return Ok(InterruptStatus(0));
                    }

                    // Clear all interrupt flags
                    self.ifcr(|w| w.all());

                    if isr.error() {
                        // On error hardware clears EN bit, we disable all interrupts
                        self.ch().cr.modify(|_, w| {
                            w
                                .htie().disabled()
                                .tcie().enabled()
                                .teie().enabled()
                        });
                    }
                    isr.as_result()
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

    /// Replace error flag with `Err`
    ///
    /// A DMA error is generated when redaing from or writing to a reserved address space.
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
    pub fn all(&mut self) -> &mut Self {
        self.0 |= 0b0001;
        self
    }

    pub fn complete(&mut self) -> &mut Self {
        self.0 |= 0b0010;
        self
    }

    pub fn half_complete(&mut self) -> &mut Self {
        self.0 |= 0b0100;
        self
    }

    pub fn error(&mut self) -> &mut Self {
        self.0 |= 0b1000;
        self
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
