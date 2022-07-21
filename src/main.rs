#![no_main]
#![no_std]

use panic_probe as _;
use defmt_rtt as _;
use stm32f0xx_hal as hal;
use ghanima as lib;

#[rtic::app(device = crate::hal::pac, dispatchers = [CEC_CAN, USART3_4])]
mod app {
    use cortex_m::interrupt::free as ifree;
    use super::hal;
    use hal::prelude::*;
    use usb_device::class_prelude::UsbBusAllocator;

    use super::lib;
    use lib::bsp::{self, debug, joystick, ws2812b, usb::Usb, sides::BoardSide};
    use lib::hal_ext::{crc, spi, uart, dma::{DmaSplit, DmaTx}};
    use lib::{keyboard, config, ioqueue};

    const DEBOUNCE_COUNT: u16 = 5;

    type SerialTx = keyboard::Transmitter<uart::Tx, 4>;
    type SerialRx = keyboard::Receiver<uart::Rx<&'static mut [u8]>, 4, 32>;
    type Leds = ws2812b::Leds<{ bsp::NLEDS }>;

    #[shared]
    struct Shared {
        usb: Usb<keyboard::leds::KeyboardLedsState>,
        spi_tx: spi::SpiTx,
        serial_tx: SerialTx,
        serial_rx: SerialRx,
        crc: crc::Crc,
        leds: keyboard::LedController<'static>,
        keyboard: keyboard::Keyboard<{ config::N_LAYERS }>,
    }

    #[local]
    struct Local {
        timer: hal::timers::Timer<hal::pac::TIM15>,
        joy: joystick::Joystick,
    }

    #[monotonic(binds = SysTick, default = true)]
    type Mono = systick_monotonic::Systick<MONO_HZ>;
    pub const MONO_HZ: u32 = 1000;

    #[init(local = [
        usb_bus: Option<UsbBusAllocator<hal::usb::UsbBusType>> = None,
        led_buf: [u8; Leds::BUFFER_SIZE] = [0; Leds::BUFFER_SIZE],
        serial_tx_buf: [u8; 64] = [0; 64],
        serial_rx_buf: [u8; 128] = [0; 128],
    ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        let mut core = cx.core;
        let mut dev = cx.device;

        // Automatically enter sleep mode when leaving an ISR
        if cfg!(feature = "idle-sleep") {
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

        // CRC
        let crc = crc::Crc::new(dev.CRC, &mut rcc);

        // Determine board side
        let board_side = ifree(|cs| gpiob.pb13.into_floating_input(cs));
        let board_side = BoardSide::get(board_side);

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
            460_800.bps(),
            &mut rcc,
        ).split();
        debug::tasks::init((debug_tx, debug_rx));

        // ADC
        let joy_x = ifree(|cs| gpioa.pa0.into_analog(cs));
        let joy_y = ifree(|cs| gpioa.pa1.into_analog(cs));
        let mut joy = joystick::Joystick::new(dev.ADC, (joy_y, joy_x), &mut rcc);

        // SPI (tx only) for RGB data
        // HAL provides only a blocking interface, so we must configure everything on our own
        let rgb_tx = ifree(|cs| gpiob.pb15.into_alternate_af0(cs));  // SPI2_MOSI
        let mut spi_tx = spi::SpiTx::new(dev.SPI2, rgb_tx, dma.ch5, &mut cx.local.led_buf[..], 3.mhz(), &mut rcc);

        // configure periodic timer
        let mut timer = hal::timers::Timer::tim15(dev.TIM15, 1.khz(), &mut rcc);
        timer.listen(hal::timers::Event::TimeOut);

        // USB
        let usb = hal::usb::Peripheral {
            usb: dev.USB,
            pin_dp: gpioa.pa12,
            pin_dm: gpioa.pa11
        };
        *cx.local.usb_bus = Some(hal::usb::UsbBus::new(usb));
        let usb_bus = cx.local.usb_bus.as_ref().unwrap();

        let usb = Usb::new(usb_bus, &board_side, Default::default());

        // Keyboard
        let serial_tx = keyboard::Transmitter::new(serial_tx);
        let serial_rx = keyboard::Receiver::new(serial_rx);
        let (keyboard, mut leds) = keyboard::Keyboard::new(
            keyboard::Keys::new(board_side, cols, rows, DEBOUNCE_COUNT),
            &config::CONFIG,
        );
        // Send a first transfer ASAP with all LEDs in initial state
        spi_tx.push(|buf| leds.tick(0).serialize_to_slice(buf)).unwrap();
        spi_tx.start().unwrap();

        if !joy.detect() {
            defmt::warn!("Joystick not detected");
        }

        debug::tasks::trace::run(|| defmt::info!("Liftoff!"));

        let shared = Shared {
            usb,
            spi_tx,
            serial_tx,
            serial_rx,
            crc,
            leds,
            keyboard,
        };

        let local = Local {
            timer,
            joy,
        };

        let mono = systick_monotonic::Systick::new(core.SYST, sysclk.0);

        (shared, local, init::Monotonics(mono))
    }

    #[task(binds = TIM15, priority = 4, local = [timer, t: u32 = 0])]
    fn tick(cx: tick::Context) {
        debug::tasks::task::enter();
        // Clears interrupt flag
        if cx.local.timer.wait().is_ok() {
            let t = cx.local.t;
            *t += 1;

            if *t % 10 == 0 {
                // ignore error if we're too slow
                if leds_tick::spawn(*t).is_err() {
                    defmt::warn!("Spawn failed: leds_tick");
                };
            }

            if *t % 10 == 0 {
                if read_joystick::spawn().is_err() {
                    defmt::warn!("Spawn failed: read_joystick");
                };
            }

            if keyboard_tick::spawn(*t).is_err() {
                defmt::error!("Spawn failed: keyboard_tick");
            }

            if *t % 1000 == 0 {
                if debug_report::spawn().is_err() {
                    defmt::warn!("Spawn failed: debug_report");
                }
            }
        }
        debug::tasks::task::exit();
    }

    /// USB poll
    ///
    /// On an USB interrput we need to handle all classes and receive/send proper data.
    /// This is always a response to USB host polling because host initializes all transactions.
    #[task(binds = USB, priority = 3, shared = [usb])]
    fn usb_poll(mut cx: usb_poll::Context) {
        debug::tasks::task::enter();
        cx.shared.usb.lock(|usb| {
            // UsbDevice.poll()->UsbBus.poll() inspects and clears USB interrupt flags.
            // If there was data packet to any class this will return true.
            let _was_packet = usb.poll();
        });
        debug::tasks::task::exit();
    }

    #[task(priority = 2, capacity = 1, shared = [serial_tx, serial_rx, crc, usb, keyboard])]
    fn keyboard_tick(cx: keyboard_tick::Context, t: u32) {
        debug::tasks::task::enter();
        let keyboard_tick::SharedResources {
            serial_tx: mut tx,
            serial_rx: rx,
            crc,
            usb,
            keyboard,
        } = cx.shared;

        // Run main keyboard logic
        let leds_update = (&mut tx, rx, usb, keyboard).lock(|tx, rx, usb, keyboard| {
            keyboard.tick((tx, rx), usb)
        });

        // Transmit any serial messages
        (tx, crc).lock(|tx, crc| tx.tick(crc));

        // Update LED patterns
        update_leds_state::spawn(t, leds_update).map_err(|_| ()).unwrap();
        debug::tasks::task::exit();
    }

    #[task(priority = 1, shared = [keyboard], local = [joy, certainty: u8 = 0])]
    fn read_joystick(mut cx: read_joystick::Context) {
        const MAX: u8 = 10;
        const MARGIN: u8 = 2;

        let certainty = cx.local.certainty;

        // When we are not certain that joystick exists use zeroes
        let xy = if *certainty >= MAX - MARGIN {
            cx.local.joy.read_xy()
        } else {
            (0, 0)
        };
        cx.shared.keyboard.lock(|kb| kb.update_joystick(xy));

        // Update joystick detection knowledge, do this _after_ ADC reading to avoid
        // messing up the readings.
        if cx.local.joy.detect() {
            *certainty = (*certainty + 1).min(MAX);
        } else {
            *certainty = certainty.saturating_sub(1);
        }
    }

    /// Apply state updates from keyboard_tick
    ///
    /// This has the same priority as update_leds but we use a queue to eventually apply all
    /// the updates.
    #[task(priority = 1, shared = [leds], capacity = 4)]
    fn update_leds_state(cx: update_leds_state::Context, t: u32, update: keyboard::LedsUpdate) {
        debug::tasks::task::enter();
        let mut leds = cx.shared.leds;
        leds.lock(|leds| update.apply(t, leds));
        debug::tasks::task::exit();
    }

    #[task(priority = 1, shared = [spi_tx, leds])]
    fn leds_tick(cx: leds_tick::Context, t: u32) {
        debug::tasks::task::enter();
        let leds_tick::SharedResources {
            mut spi_tx,
            mut leds,
        } = cx.shared;

        leds.lock(|leds| {
            // Get new LED colors
            let colors = debug::tasks::trace::run(|| leds.tick(t));

            // Prepare data to be sent and start DMA transfer.
            // `leds` must be kept locked because we're serializing from reference.
            spi_tx.lock(|spi_tx| {
                debug::tasks::trace::run(|| {
                    // TODO: try to use .serialize()
                    spi_tx.push(|buf| colors.serialize_to_slice(buf))
                        .expect("Trying to serialize new data but DMA transfer is not finished");
                });

                 spi_tx.start()
                    .expect("If we were able to serialize we must be able to start!");
                debug::tasks::trace::start();
            });
        });
        debug::tasks::task::exit();
    }


    #[task(priority = 1, shared = [serial_rx], local = [stats: Option<ioqueue::Stats> = None])]
    fn debug_report(mut cx: debug_report::Context) {
        debug::tasks::task::enter();
        let old = cx.local.stats.get_or_insert_with(|| Default::default());
        let new = cx.shared.serial_rx.lock(|rx| {
            rx.stats().clone()
        });
        if &new != old {
            defmt::warn!("RX stats: {}", new);
            *old = new;
        }
        debug::tasks::task::exit();
    }

    #[task(binds = DMA1_CH4_5_6_7, priority = 4, shared = [spi_tx])]
    fn dma_spi_callback(mut cx: dma_spi_callback::Context) {
        debug::tasks::task::enter();
        cx.shared.spi_tx.lock(|spi_tx| {
           spi_tx.on_interrupt()
               .as_option()
               .transpose()
               .expect("Unexpected interrupt");
        });
        debug::tasks::trace::end();
        debug::tasks::task::exit();
    }

    #[task(binds = DMA1_CH2_3, priority = 4, shared = [crc, serial_tx, serial_rx])]
    fn dma_uart_callback(cx: dma_uart_callback::Context) {
        debug::tasks::task::enter();
        let tx = cx.shared.serial_tx;
        let rx = cx.shared.serial_rx;
        let crc = cx.shared.crc;
        (tx, rx, crc).lock(|tx, rx, mut crc| {
            let rx_done = rx.on_interrupt(&mut crc)
                .as_option().transpose().expect("Unexpected interrupt");
            let tx_done = tx.on_interrupt()
                .as_option().transpose().expect("Unexpected interrupt");

            if rx_done.is_some() {
                defmt::trace!("UART RX done");
            }

            if tx_done.is_some() {
                defmt::trace!("UART TX done");
            }

            rx_done.or(tx_done).expect("No interrupt handled!");
        });
        debug::tasks::task::exit();
    }

    #[task(binds = USART1, priority = 4, shared = [crc, serial_rx], local = [
           empty_count: usize = 0,
    ])]
    fn uart_interrupt(cx: uart_interrupt::Context) {
        debug::tasks::task::enter();
        let rx = cx.shared.serial_rx;
        let crc = cx.shared.crc;
        (rx, crc).lock(|rx, mut crc| {
            rx.on_interrupt(&mut crc)
                .as_option().transpose().expect("Unexpected interrupt");
        });
        debug::tasks::task::exit();
    }

    #[idle]
    fn idle(_cx: idle::Context) -> ! {
        loop {
            if cfg!(feature = "debug-tasks") {
                debug::tasks::task::idle();
            }
            if cfg!(feature = "idle-sleep") {
                rtic::export::wfi();
            } else {
                rtic::export::nop();
            }
        }
    }
}
