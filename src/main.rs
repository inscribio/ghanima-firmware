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
    use crate::{spi, usb::Usb, board::BoardSide, utils::InfallibleResult};
    use ghanima::ws2812b;
    use hal::{prelude::*, serial::Serial, adc};
    use cortex_m::interrupt::free as ifree;
    use usb_device::{prelude::*, class_prelude::UsbBusAllocator};
    use core::fmt::Write as _;

    #[shared]
    struct Shared {
        usb: Usb,
        ws2812: ws2812b::Leds,
        spi_tx: spi::SpiTransfer<&'static mut [u8]>,
        dbg_pin: hal::gpio::Pin<hal::gpio::Output<hal::gpio::PushPull>>
    }

    #[local]
    struct Local {
        timer: hal::timers::Timer<hal::pac::TIM15>,
    }

    #[init(local = [
        usb_bus: Option<UsbBusAllocator<hal::usb::UsbBusType>> = None,
        led_buf: ws2812b::Buffer = ws2812b::BUFFER_ZERO,
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
        // let board_side_pin = ifree(|cs| gpiob.pb13.into_floating_input(cs));
        // let board_side = BoardSide::get(board_side_pin);
        let board_side = BoardSide::Left;

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
        // let board_serial = Serial::usart1(dev.USART1, (board_tx, board_rx), 115_200.bps(), &mut rcc);
        let debug_serial = Serial::usart2(dev.USART2, (debug_tx, debug_rx), 115_200.bps(), &mut rcc);

        let dbg_pin = ifree(|cs| board_tx.into_push_pull_output(cs)).downgrade();

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
        let spi = spi::SpiTx::new(dev.SPI2, dev.DMA1, rgb_tx, 3.mhz(), &mut rcc);
        let spi_tx = spi.with_buf(&mut cx.local.led_buf[..]);

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
        // let usb_cdc = usbd_serial::CdcAcmClass::new(usb_bus, 64);
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

        // configure periodic timer
        let mut timer = hal::timers::Timer::tim15(dev.TIM15, 1.khz(), &mut rcc);
        timer.listen(hal::timers::Event::TimeOut);

        let shared = Shared {
            usb: Usb {
                dev: usb_dev,
                serial: usb_serial,
                // cdc: usb_cdc,
                dfu: usb_dfu,
            },
            ws2812: ws2812b::Leds::new(),
            spi_tx,
            dbg_pin,
        };

        let local = Local {
            timer,
        };

        (shared, local, init::Monotonics())
    }

    #[task(shared = [ws2812, spi_tx])]
    fn send_ws2812(mut cx: send_ws2812::Context) {
        (cx.shared.ws2812,cx.shared.spi_tx).lock(|ws2812, spi_tx| {
            // fill the buffer; when this task is started dma must already be finished
            // TODO: try to use .serialize()
            ws2812.serialize_to_slice(spi_tx.take().unwrap());
            // start the transfer
            spi_tx.start();
        });
    }

    #[task(binds = DMA1_CH4_5_6_7, priority = 3, shared = [spi_tx])]
    fn dma_complete(mut cx: dma_complete::Context) {
        cx.shared.spi_tx.lock(|spi_tx| {
            if !spi_tx.finish().unwrap() {
                panic!("Interrupt from unexpected channel");
            }
        });
    }

    fn gamma_correction(pixel: u8) -> u8 {
        // https://docs.rs/smart-leds/0.3.0/src/smart_leds/lib.rs.html#43-45
        const GAMMA: [u8; 256] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4,
            4, 4, 4, 5, 5, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7, 8, 8, 8, 9, 9, 9, 10, 10, 10, 11, 11, 11,
            12, 12, 13, 13, 13, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21, 21, 22,
            22, 23, 24, 24, 25, 25, 26, 27, 27, 28, 29, 29, 30, 31, 32, 32, 33, 34, 35, 35, 36, 37,
            38, 39, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 50, 51, 52, 54, 55, 56, 57, 58,
            59, 60, 61, 62, 63, 64, 66, 67, 68, 69, 70, 72, 73, 74, 75, 77, 78, 79, 81, 82, 83, 85,
            86, 87, 89, 90, 92, 93, 95, 96, 98, 99, 101, 102, 104, 105, 107, 109, 110, 112, 114,
            115, 117, 119, 120, 122, 124, 126, 127, 129, 131, 133, 135, 137, 138, 140, 142, 144,
            146, 148, 150, 152, 154, 156, 158, 160, 162, 164, 167, 169, 171, 173, 175, 177, 180,
            182, 184, 186, 189, 191, 193, 196, 198, 200, 203, 205, 208, 210, 213, 215, 218, 220,
            223, 225, 228, 231, 233, 236, 239, 241, 244, 247, 249, 252, 255,
        ];
        GAMMA[pixel as usize]
    }

    #[task(binds = TIM15, priority = 2, shared = [dbg_pin], local = [timer, t: usize = 0])]
    fn tick(mut cx: tick::Context) {
        *cx.local.t += 1;

        // Clears interrupt flag
        if cx.local.timer.wait().is_ok() {
            let period_ms = 50;
            if *cx.local.t % period_ms == 0 {
                send_ws2812::spawn().unwrap();
            }
        }
    }

    /// USB poll
    ///
    /// On an USB interrput we need to handle all classes and receive/send proper data.
    /// This is always a response to USB host polling because host initializes all transactions.
    #[task(binds = USB, priority = 2, shared = [usb])]
    fn usb_poll(mut cx: usb_poll::Context) {
        cx.shared.usb.lock(|usb| {
            // UsbDevice.poll()->UsbBus.poll() inspects and clears USB interrupt flags.
            // If there was data packet to any class this will return true.
            let _was_packet = usb.poll();
            usb.serial.flush().ok();
        });
    }

    #[idle]
    fn idle(mut _cx: idle::Context) -> ! {
        loop {
            if cfg!(feature = "idle_sleep") {
                rtic::export::wfi();
            } else {
                rtic::export::nop();
            }
        }
    }
}
