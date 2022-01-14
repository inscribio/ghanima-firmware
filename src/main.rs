#![no_main]
#![no_std]

use core::convert::Infallible;

use bitfield::Bit;
use panic_halt as _;
use stm32f0 as _;
use stm32f0xx_hal as hal;

use hal::{prelude::*, usb};
use embedded_hal::digital::v2::InputPin;
use usb_device::prelude::UsbDevice;
use usbd_hid::hid_class::HIDClass;
use usbd_dfu_rt::DfuRuntimeClass;

use keyberon;

mod reboot;
mod utils;
use utils::InfallibleResult;

pub struct Usb {
    dev: UsbDevice<'static, usb::UsbBusType>,
    serial: usbd_serial::SerialPort<'static, usb::UsbBusType>,
    // keyboard: keyberon::Class<'static, usb::UsbBusType, ()>,
    // mouse: HIDClass<'static, usb::UsbBusType>,
    // this does not need to be share but it should be cleaner to have it here
    dfu: DfuRuntimeClass<reboot::DfuBootloader>,
}

impl Usb {
    pub fn poll(&mut self) -> bool {
        self.dev.poll(&mut [&mut self.serial, &mut self.dfu])
    }
}

const NCOLS: usize = 6;
const NROWS: usize = 5;

pub enum BoardSide {
    Left,
    Right,
}

impl BoardSide {
    /// Board side can be determined via pull-up/down on a pin
    pub fn get(pin: impl InputPin<Error = Infallible>) -> Self {
        if pin.is_high().infallible() {
            Self::Left
        } else {
            Self::Right
        }
    }

    /// Keyboard matrix coordinates have to be transformed to global representation
    pub fn transform_coordinates(&self, (row, col): (u8, u8)) -> (u8, u8) {
        match self {
            Self::Left => (row, col),
            Self::Right => (row, 2 * NCOLS as u8 - 1 - col),
        }
    }
}

/// TX only, asynchronious SPI implementation
///
/// Implementation that uses SPI2 to just send arbitrary data. MISO/SCK pins are not used.
struct Spi2Tx {
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

#[rtic::app(device = crate::hal::pac, dispatchers = [CEC_CAN])]
mod app {
    use super::{hal, Spi2Tx, BoardSide, Usb};
    use hal::{prelude::*, serial::Serial, adc};
    use cortex_m::interrupt::free as ifree;
    use usb_device::{prelude::*, class_prelude::UsbBusAllocator};

    #[shared]
    struct Shared {
        // time_ms: u32,
        // leds: Leds,
        usb: Usb,
        // do_reboot: bool,
    }

    #[local]
    struct Local {
        // timer: hal::timers::Timer<hal::pac::TIM15>,
        // matrix: Matrix<Pin<Input<PullUp>>, Pin<Output<PushPull>>, NCOLS, NROWS>,
        // debouncer: Debouncer<PressedKeys<NCOLS, NROWS>>,
        // layout: Layout<CustomAction>,
        // uart_tx: serial::Tx<hal::pac::USART1>,
        // uart_rx: serial::Rx<hal::pac::USART1>,
    }

    #[init(local = [
        usb_bus: Option<UsbBusAllocator<hal::usb::UsbBusType>> = None,
    ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        let mut core = cx.core;
        let mut dev = cx.device;

        // Automatically enter sleep mode when leaving an ISR
        if cfg!(feature = "idle_sleep") {
            core.SCB.set_sleeponexit();
        }

        // Clock configuration (may use external crystal, but it is not needed for STM32F072)
        let sysclk: hal::time::Hertz = 48.mhz().into();
        let pclk: hal::time::Hertz = 24.mhz().into();
        let crystal_clk: hal::time::Hertz = 12.mhz().into();

        let clk_config = dev.RCC
            .configure()
            .enable_crs(dev.CRS) // synchronization to USB SOF
            .sysclk(sysclk)
            .pclk(pclk);
        let clk_config = if cfg!(feature = "crystal") {
            clk_config.hse(crystal_clk, hal::rcc::HSEBypassMode::NotBypassed)
        } else {
            clk_config.hsi48()
        };
        let mut rcc = clk_config.freeze(&mut dev.FLASH);

        // Pinout
        let gpioa = dev.GPIOA.split(&mut rcc);
        let gpiob = dev.GPIOB.split(&mut rcc);
        let gpioc = dev.GPIOC.split(&mut rcc);

        // TODO: configure debug pins, verify that SWD works by default

        // Determine board side
        let board_side_pin = ifree(|cs| gpiob.pb13.into_floating_input(cs));
        let board_side = BoardSide::get(board_side_pin);

        // Keyboard matrix
        let cols = ifree(|cs| [
            gpiob.pb1.into_pull_up_input(cs).downgrade(),
            gpiob.pb0.into_pull_up_input(cs).downgrade(),
            gpioa.pa7.into_pull_up_input(cs).downgrade(),
            gpioa.pa6.into_pull_up_input(cs).downgrade(),
            gpioa.pa5.into_pull_up_input(cs).downgrade(),
            gpioa.pa4.into_pull_up_input(cs).downgrade(),
        ]);
        let rows =  ifree(|cs| [
            gpiob.pb6.into_push_pull_output(cs).downgrade(),
            gpiob.pb7.into_push_pull_output(cs).downgrade(),
            gpioc.pc13.into_push_pull_output(cs).downgrade(),
            gpioc.pc14.into_push_pull_output(cs).downgrade(),
            gpioc.pc15.into_push_pull_output(cs).downgrade(),
        ]);

        // UARTs
        let board_tx = ifree(|cs| gpioa.pa9.into_alternate_af1(cs));
        let board_rx = ifree(|cs| gpioa.pa10.into_alternate_af1(cs));
        let debug_tx = ifree(|cs| gpioa.pa2.into_alternate_af1(cs));
        let debug_rx = ifree(|cs| gpioa.pa3.into_alternate_af1(cs));
        let board_serial = Serial::usart1(dev.USART1, (board_tx, board_rx), 115_200.bps(), &mut rcc);
        let debug_serial = Serial::usart2(dev.USART2, (debug_tx, debug_rx), 115_200.bps(), &mut rcc);

        // ADC
        // Dedicated 14 MHz clock source is used. Conversion time is:
        // t_conv = (239.5 + 12.5) * (1/14e6) ~= 18 us
        let joy_x = ifree(|cs| gpioa.pa0.into_analog(cs));
        let joy_y = ifree(|cs| gpioa.pa1.into_analog(cs));
        let mut joy_adc = adc::Adc::new(dev.ADC, &mut rcc);
        joy_adc.set_align(adc::AdcAlign::Right);
        joy_adc.set_precision(adc::AdcPrecision::B_12);
        joy_adc.set_sample_time(adc::AdcSampleTime::T_239);

        // SPI (tx only) for RGB data
        // HAL provides only a blocking interface, so we must configure everything on our own
        let rgb_tx = ifree(|cs| gpiob.pb15.into_alternate_af0(cs));  // SPI2_MOSI
        let spi = Spi2Tx::new(dev.SPI2, rgb_tx);

        // USB
        let usb = hal::usb::Peripheral {
            usb: dev.USB,
            pin_dp: gpioa.pa12,
            pin_dm: gpioa.pa11
        };
        *cx.local.usb_bus = Some(hal::usb::UsbBus::new(usb));
        let usb_bus = cx.local.usb_bus.as_ref().unwrap();

        // USB classes
        let usb_serial = usbd_serial::SerialPort::new(usb_bus);
        let usb_dfu = usbd_dfu_rt::DfuRuntimeClass::new(usb_bus, crate::reboot::DfuBootloader);

        // TODO: follow guidelines from https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt
        // VID:PID recognised as Van Ooijen Technische Informatica:Keyboard
        let generic_keyboard = UsbVidPid(0x16c0, 0x27db);
        let usb_dev = UsbDeviceBuilder::new(&usb_bus, generic_keyboard)
            .manufacturer("inscrib.io")
            .product(match board_side {
                BoardSide::Left => "ghanima keyboard (L)",
                BoardSide::Right => "ghanima keyboard (R)"
            })
            .serial_number(env!("CARGO_PKG_VERSION"))
            .composite_with_iads()
            .build();

        (Shared {
            usb: Usb {
                dev: usb_dev,
                serial: usb_serial,
                dfu: usb_dfu,
            },
        }, Local {}, init::Monotonics())
    }

    /// USB poll
    ///
    /// On an USB interrput we need to handle all classes and receive/send proper data.
    /// This is always a response to USB host polling because host initializes all transactions.
    #[task(binds = USB, shared = [usb])]
    fn usb_poll(mut cx: usb_poll::Context) {
        cx.shared.usb.lock(|usb| {
            // UsbDevice.poll()->UsbBus.poll() inspects and clears USB interrupt flags.
            // If there was data packet to any class this will return true.
            let _was_packet = usb.poll();

            // debugging
            if _was_packet {
                let mut buf = [0u8; 64];

                match usb.serial.read(&mut buf) {
                    Ok(count) if count > 0 => {
                        // toggle case
                        for c in buf[..count].iter_mut() {
                            if c.is_ascii_uppercase() {
                                c.make_ascii_lowercase();
                            } else {
                                c.make_ascii_uppercase();
                            }
                        }

                        // send back
                        let mut write_offset = 0;
                        while write_offset < count {
                            match usb.serial.write(&buf[write_offset..count]) {
                                Ok(len) if len > 0 => write_offset += len,
                                _ => {},
                            }
                        }
                    },
                    _ => {},
                }
            }
        });
    }

    #[idle]
    fn idle(_cx: idle::Context) -> ! {
        loop {
            if cfg!(feature = "idle_sleep") {
                rtic::export::wfi();
            } else {
                rtic::export::nop();
            }
        }
    }
}
