#![no_main]
#![no_std]

mod board;
mod reboot;
mod spi;
mod usb;
mod utils;

use panic_probe as _;
use defmt_rtt as _;
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

        let mut dbg_pin = ifree(|cs| gpioa.pa9.into_push_pull_output(cs)).downgrade();
        for _ in 0..3 {
            dbg_pin.set_high().infallible();
            cortex_m::asm::delay(48);
            dbg_pin.set_low().infallible();
            cortex_m::asm::delay(48);
        }

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
        // let board_tx = ifree(|cs| gpioa.pa9.into_alternate_af1(cs));
        let board_rx = ifree(|cs| gpioa.pa10.into_alternate_af1(cs));
        let debug_tx = ifree(|cs| gpioa.pa2.into_alternate_af1(cs));
        let debug_rx = ifree(|cs| gpioa.pa3.into_alternate_af1(cs));
        // let board_serial = Serial::usart1(dev.USART1, (board_tx, board_rx), 115_200.bps(), &mut rcc);
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
        let spi = spi::SpiTx::new(dev.SPI2, dev.DMA1, rgb_tx, 3.mhz(), &mut rcc);
        let mut spi_tx = spi.with_buf(&mut cx.local.led_buf[..]);
        let mut ws2812 = ws2812b::Leds::new();
        // Send a first transfer with all leds disabled ASAP
        ws2812.serialize_to_slice(spi_tx.take().unwrap());
        spi_tx.start();

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

        dbg_pin.set_high().infallible();
        defmt::info!("Liftoff!");
        dbg_pin.set_low().infallible();

        let shared = Shared {
            usb: Usb {
                dev: usb_dev,
                serial: usb_serial,
                // cdc: usb_cdc,
                dfu: usb_dfu,
            },
            ws2812,
            spi_tx,
            dbg_pin,
        };

        let local = Local {
            timer,
        };

        (shared, local, init::Monotonics())
    }

    #[task(shared = [ws2812, spi_tx, dbg_pin])]
    fn send_ws2812(mut cx: send_ws2812::Context) {
        (cx.shared.ws2812,cx.shared.spi_tx).lock(|ws2812, spi_tx| {
            // fill the buffer; when this task is started dma must already be finished
            cx.shared.dbg_pin.lock(|pin| pin.set_high().infallible());
            // TODO: try to use .serialize()
            ws2812.serialize_to_slice(spi_tx.take().unwrap());
            cx.shared.dbg_pin.lock(|pin| pin.set_low().infallible());
            // start the transfer
            spi_tx.start();
            cx.shared.dbg_pin.lock(|pin| pin.set_high().infallible());
        });
    }

    #[task(binds = DMA1_CH4_5_6_7, priority = 3, shared = [spi_tx, dbg_pin])]
    fn dma_complete(mut cx: dma_complete::Context) {
        cx.shared.spi_tx.lock(|spi_tx| {
            if !spi_tx.finish().unwrap() {
                defmt::panic!("Interrupt from unexpected channel");
            }
        });
        cx.shared.dbg_pin.lock(|pin| pin.set_low().infallible());
    }

    #[task(binds = TIM15, priority = 2, shared = [ws2812, dbg_pin], local = [timer, t: usize = 0])]
    fn tick(mut cx: tick::Context) {
        // cx.shared.dbg_pin.lock(|pin| pin.toggle().infallible());
        *cx.local.t += 1;

        // Clears interrupt flag
        if cx.local.timer.wait().is_ok() {
            let period_ms = 10;

            if *cx.local.t % period_ms == 0 {
                cx.shared.dbg_pin.lock(|pin| pin.set_high().infallible());
                cx.shared.ws2812.lock(|ws2812| {
                    ws2812.set_test_pattern(*cx.local.t / period_ms, 100);
                });
                cx.shared.dbg_pin.lock(|pin| pin.set_low().infallible());

                cx.shared.dbg_pin.lock(|pin| pin.set_high().infallible());
                defmt::info!("Sending at {=u32} ms", *cx.local.t as u32);
                cx.shared.dbg_pin.lock(|pin| pin.set_low().infallible());

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
