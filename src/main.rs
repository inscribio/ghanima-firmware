#![no_main]
#![no_std]

use panic_probe as _;
use defmt_rtt as _;
use stm32f0 as _;
use stm32f0xx_hal as hal;
use ghanima as lib;

#[rtic::app(device = crate::hal::pac, dispatchers = [CEC_CAN])]
mod app {
    use cortex_m::interrupt::free as ifree;
    use super::hal;
    use hal::{prelude::*, serial::Serial};
    use usb_device::{prelude::*, class_prelude::UsbBusAllocator};

    use super::lib;
    use lib::bsp::{debug, joystick, ws2812b, usb::Usb, sides::BoardSide};
    use lib::hal_ext::{spi, uart, dma::DmaSplit, reboot};
    use lib::utils::InfallibleResult;

    #[shared]
    struct Shared {
        usb: Usb,
        ws2812: ws2812b::Leds,
        spi_tx: spi::SpiTransfer<&'static mut [u8]>,
        dbg: debug::DebugPins,
        joy: joystick::Joystick,
        serial_tx: uart::Tx,
        serial_rx: uart::Rx<&'static mut [u8]>,
    }

    #[local]
    struct Local {
        timer: hal::timers::Timer<hal::pac::TIM15>,
    }

    #[init(local = [
        usb_bus: Option<UsbBusAllocator<hal::usb::UsbBusType>> = None,
        led_buf: ws2812b::Buffer = ws2812b::BUFFER_ZERO,
        serial_tx_buf: [u8; 64] = [0; 64],
        serial_rx_buf: [u8; 128] = [0; 128],
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

        // DMA
        let dma = dev.DMA1.split(&mut rcc);

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
        let (serial_tx, serial_rx) = uart::Uart::new(
            dev.USART1,
            (board_tx, board_rx),
            (dma.ch2, dma.ch3),
            (&mut cx.local.serial_tx_buf[..], &mut cx.local.serial_rx_buf[..]),
            115_200.bps(),
            &mut rcc,
        ).split();
        let mut dbg = debug::DebugPins::new(dev.USART2, (debug_tx, debug_rx), &mut rcc);

        // ADC
        let joy_x = ifree(|cs| gpioa.pa0.into_analog(cs));
        let joy_y = ifree(|cs| gpioa.pa1.into_analog(cs));
        let mut joy = joystick::Joystick::new(dev.ADC, (joy_y, joy_x), &mut rcc);

        // SPI (tx only) for RGB data
        // HAL provides only a blocking interface, so we must configure everything on our own
        let rgb_tx = ifree(|cs| gpiob.pb15.into_alternate_af0(cs));  // SPI2_MOSI
        let spi = spi::SpiTx::new(dev.SPI2, dma.ch5, rgb_tx, 3.mhz(), &mut rcc);
        let mut spi_tx = spi.with_buf(&mut cx.local.led_buf[..]);
        let mut ws2812 = ws2812b::Leds::new();
        // Send a first transfer with all leds disabled ASAP
        ws2812.serialize_to_slice(spi_tx.take().unwrap());
        spi_tx.start().unwrap();

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
        let usb_dfu = usbd_dfu_rt::DfuRuntimeClass::new(usb_bus, reboot::DfuBootloader);

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

        dbg.with_tx_high(|| {
            defmt::info!("Liftoff!");
        });

        if !joy.detect() {
            defmt::warn!("Joystick not detected");
        }

        let shared = Shared {
            usb: Usb {
                dev: usb_dev,
                serial: usb_serial,
                // cdc: usb_cdc,
                dfu: usb_dfu,
            },
            ws2812,
            spi_tx,
            dbg,
            joy,
            serial_tx,
            serial_rx,
        };

        let local = Local {
            timer,
        };

        (shared, local, init::Monotonics())
    }

    #[task(shared = [ws2812, spi_tx, dbg])]
    fn send_ws2812(mut cx: send_ws2812::Context) {
        let send_ws2812::SharedResources {
            ws2812,
            spi_tx,
            dbg
        } = cx.shared;

        (ws2812, spi_tx, dbg).lock(|ws2812, spi_tx, dbg| {
            // fill the buffer; when this task is started dma must already be finished
            // TODO: try to use .serialize()
            dbg.with_rx_high(|| {
                ws2812.serialize_to_slice(spi_tx.take().unwrap());
            });

            // start the transfer
            if spi_tx.start().is_ok() {
                dbg.set_tx(true);
            }
        });
    }

    #[task(binds = DMA1_CH4_5_6_7, priority = 3, shared = [spi_tx, dbg])]
    fn dma_complete(mut cx: dma_complete::Context) {
        cx.shared.spi_tx.lock(|spi_tx| {
            if !spi_tx.finish().unwrap() {
                defmt::panic!("Interrupt from unexpected channel");
            }
        });
        cx.shared.dbg.lock(|d| d.set_tx(false));
    }

    #[task(binds = DMA1_CH2_3, priority = 3, shared = [serial_tx, serial_rx])]
    fn dma_complete_123(mut cx: dma_complete_123::Context) {
        cx.shared.serial_rx.lock(|rx| {
            rx.on_transfer_complete().unwrap();
        });
        cx.shared.serial_tx.lock(|tx| {
            tx.finish().unwrap();
        });
    }

    #[task(binds = USART1, priority = 3, shared = [serial_rx], local = [
           empty_count: usize = 0,
    ])]
    fn uart_interrupt(mut cx: uart_interrupt::Context) {
        cx.shared.serial_rx.lock(|rx| {
            if let Some(d) = rx.on_uart_interrupt() {
                if d.data1.len() == 0 && d.data2.len() == 0 {
                    *cx.local.empty_count += 1;
                } else {
                    defmt::info!("RX: '{=str}' + '{=str}', lost {=usize}, empty_count = {=usize}",
                        core::str::from_utf8(d.data1).unwrap_or("<WRONG_UTF8>"),
                        core::str::from_utf8(d.data2).unwrap_or("<WRONG_UTF8>"),
                        d.overwritten,
                        *cx.local.empty_count,
                    );
                }
            }
        });
    }

    #[task(
        binds = TIM15, priority = 2,
        shared = [ws2812, dbg, joy, usb, serial_tx],
        local = [timer, t: usize = 0]
    )]
    fn tick(mut cx: tick::Context) {
        // cx.shared.dbg.lock(|pin| pin.toggle().infallible());
        *cx.local.t += 1;

        // Clears interrupt flag
        if cx.local.timer.wait().is_ok() {
            let period_ms = 10;

            if *cx.local.t % period_ms == 0 {
                cx.shared.dbg.lock(|dbg| {

                    let (r, angle) = dbg.with_tx_high(|| {
                        cx.shared.joy.lock(|j| j.read_polar())
                    });

                    let brightness = dbg.with_rx_high(|| {
                        if r > 300.0 {
                            ((angle / 4.0).min(1.0).max(0.0) * 255 as f32) as u8
                        } else {
                            0
                        }
                    });

                    // defmt::info!("brightness = {=u8}", brightness);

                    dbg.with_tx_high(|| {
                        cx.shared.ws2812.lock(|ws2812| {
                            // ws2812.set_test_pattern(*cx.local.t / period_ms, 100);
                            ws2812.set_test_pattern(*cx.local.t / period_ms, brightness);
                        });
                    });
                });

                send_ws2812::spawn().unwrap();
            }

            if *cx.local.t % 333 == 0 {
                cx.shared.serial_tx.lock(|s| {
                    let msg = "Hello world";
                    // defmt::info!("Transmitting ...");
                    s.transmit(msg.as_bytes()).unwrap();
                });
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
