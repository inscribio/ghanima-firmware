use crate::hal;

/// Extension trait to split DMA into separate channels
pub trait DmaSplit {
    /// Structure holding DMA channles
    type Channels;

    /// Split DMA into independent channels
    fn split(self, rcc: &mut hal::rcc::Rcc) -> Self::Channels;
}

pub struct DmaChannel<const C: u8>;
pub struct InterruptStatus(u8);
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

    fn split(self, rcc: &mut hal::rcc::Rcc) -> Self::Channels {
        // Need to access some registers outside of HAL type system (field `regs` is private)
        let rcc_regs = unsafe { &*hal::pac::RCC::ptr() };

        // Enable DMA clock
        rcc_regs.ahbenr.modify(|_, w| w.dmaen().enabled());

        let dma = unsafe { &*hal::pac::DMA1::ptr() };

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
    pub fn error(&self) -> bool {
        (self.0 & 0b1000) != 0
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
