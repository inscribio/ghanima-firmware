#![no_main]
#![no_std]

mod board;
mod reboot;
mod spi;
mod usb;
mod utils;

use panic_halt as _;
use stm32f0 as _;
use stm32f0xx_hal as hal;

#[rtic::app(device = crate::hal::pac, dispatchers = [CEC_CAN])]
mod app {
    use super::hal;
    use crate::{spi::Spi2Tx, usb::Usb, board::BoardSide};
    use hal::{prelude::*, serial::Serial, adc};
    use cortex_m::interrupt::free as ifree;
    use usb_device::{prelude::*, class_prelude::UsbBusAllocator};

    #[shared]
    struct Shared {
        usb: Usb,
    }

    #[local]
    struct Local {}

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

        let shared = Shared {
            usb: Usb {
                dev: usb_dev,
                serial: usb_serial,
                dfu: usb_dfu,
            },
        };

        (shared, Local {}, init::Monotonics())
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
                usb.serial_loopback(|c| {
                    if c.is_ascii_uppercase() {
                        c.make_ascii_lowercase();
                    } else {
                        c.make_ascii_uppercase();
                    }
                });
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
