#![no_main]
#![no_std]

use panic_probe as _;
use defmt_rtt as _;
use stm32f0 as _;
use stm32f0xx_hal as hal;
use ghanima as lib;

#[rtic::app(device = crate::hal::pac, dispatchers = [CEC_CAN, USART3_4])]
mod app {
    use cortex_m::interrupt::free as ifree;
    use super::hal;
    use hal::prelude::*;
    use usb_device::{prelude::*, class_prelude::UsbBusAllocator};

    use super::lib;
    use lib::bsp::{self, debug, joystick, ws2812b, usb::Usb, sides::BoardSide};
    use lib::hal_ext::{crc, spi, uart, dma::{DmaSplit, DmaTx}};
    use lib::{keyboard, layers, ioqueue};

    const DEBOUNCE_COUNT: u16 = 5;

    type SerialTx = keyboard::Transmitter<uart::Tx, 4>;
    type SerialRx = keyboard::Receiver<uart::Rx<&'static mut [u8]>, 4, 32>;
    type Leds = ws2812b::Leds<{ bsp::NLEDS }>;

    #[shared]
    struct Shared {
        usb: Usb<keyboard::leds::KeyboardLedsState>,
        dbg: debug::DebugPins,
        joy: joystick::Joystick,
        spi_tx: spi::SpiTx,
        serial_tx: SerialTx,
        serial_rx: SerialRx,
        crc: crc::Crc,
        leds: keyboard::KeyboardLeds,
    }

    #[local]
    struct Local {
        timer: hal::timers::Timer<hal::pac::TIM15>,
        keyboard: keyboard::Keyboard,
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
        let mut dbg = debug::DebugPins::new(dev.USART2, (debug_tx, debug_rx), &mut rcc);

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

        let usb = Usb::new(usb_bus, &board_side, keyboard::leds::KeyboardLedsState::new());

        // Keyboard
        let serial_tx = keyboard::Transmitter::new(serial_tx);
        let serial_rx = keyboard::Receiver::new(serial_rx);
        let keyboard = keyboard::Keyboard::new(
            keyboard::Keys::new(board_side, cols, rows, DEBOUNCE_COUNT),
            layers::layout(),
            1000,
        );

        // Keyboard RGB LEDs controller
        let mut leds = keyboard::KeyboardLeds::new(board_side, layers::led_configs());
        // Send a first transfer ASAP with all LEDs in initial state
        spi_tx.push(|buf| {
            leds.controller_mut()
                .tick(0)
                .serialize_to_slice(buf)
        }).unwrap();
        spi_tx.start().unwrap();

        dbg.with_tx_high(|| {
            defmt::info!("Liftoff!");
            defmt::debug!("Size of: layout={=usize} led_configs={=usize}",
                core::mem::size_of_val(&layers::layout()),
                core::mem::size_of_val(layers::led_configs()),
            );
        });

        if !joy.detect() {
            defmt::warn!("Joystick not detected");
        }

        let shared = Shared {
            usb,
            spi_tx,
            dbg,
            joy,
            serial_tx,
            serial_rx,
            crc,
            leds,
        };

        let local = Local {
            timer,
            keyboard,
        };

        let mono = systick_monotonic::Systick::new(core.SYST, sysclk.0);

        (shared, local, init::Monotonics(mono))
    }

    #[task(binds = TIM15, priority = 3, local = [timer, t: usize = 0])]
    fn tick(cx: tick::Context) {
        // Clears interrupt flag
        if cx.local.timer.wait().is_ok() {
            let t = cx.local.t;
            *t += 1;

            if *t % 10 == 0 {
                // ignore error if we're too slow
                update_leds::spawn(*t).ok();
            }

            keyboard_tick::spawn(*t).unwrap();

            if *t % 1000 == 0 {
                debug_report::spawn().unwrap();
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

    #[task(priority = 2, shared = [serial_tx, serial_rx, crc, usb], local = [keyboard])]
    fn keyboard_tick(cx: keyboard_tick::Context, t: usize) {
        let keyboard_tick::SharedResources {
            serial_tx: mut tx,
            serial_rx: rx,
            crc,
            mut usb,
        } = cx.shared;
        let keyboard = cx.local.keyboard;

        // Retrieve current USB state
        let (usb_state, usb_leds) = usb.lock(|usb| {
            (usb.dev.state(), *usb.keyboard_leds())
        });
        let usb_on = usb_state == UsbDeviceState::Configured;

        // Run keyboard logic and get the USB report
        let report = (&mut tx, rx).lock(|tx, rx| keyboard.tick(tx, rx, usb_on));

        // Update LED patterns
        let state = keyboard::leds::KeyboardState {
            leds: usb_leds,
            usb_on,
            role: keyboard.role(),
            layer: 0,  // FIXME: get current keyberon layer number
        };
        update_led_patterns::spawn(t, state).unwrap();

        // Transmit any serial messages
        (tx, crc).lock(|tx, crc| tx.tick(crc));

        // Set current USB report to the new one, finish if there is no change
        if !usb_on || !usb.lock(|usb| usb.keyboard.device_mut().set_keyboard_report(report.clone())) {
            return;
        }
        // Spin until we are able to send the report. Important: lock separately in each loop iterations!
        while let Ok(0) = usb.lock(|usb| usb.keyboard.write(report.as_bytes())) {}
    }

    /// Apply state updates from keyboard_tick
    ///
    /// This has the same priority as update_leds but we use a queue to eventually apply all
    /// the updates.
    #[task(priority = 1, shared = [leds], capacity = 4)]
    fn update_led_patterns(cx: update_led_patterns::Context, t: usize, state: keyboard::leds::KeyboardState) {
        let mut leds = cx.shared.leds;
        leds.lock(|leds| {
            leds.controller_mut()
                .update_patterns(t as u32, &state)
        });
    }

    #[task(priority = 1, shared = [spi_tx, dbg, leds])]
    fn update_leds(cx: update_leds::Context, t: usize) {
        let update_leds::SharedResources {
            spi_tx,
            mut dbg,
            mut leds,
        } = cx.shared;

        // Get new LED colors
        leds.lock(|leds| {
            let colors = dbg.lock(|dbg| dbg.with_tx_high(|| {
                leds.controller_mut().tick(t as u32)
            }));

            // Prepare data to send and start DMA transfer
            (spi_tx, dbg).lock(|spi_tx, dbg| {
                dbg.with_tx_high(|| {
                    // TODO: try to use .serialize()
                    spi_tx.push(|buf| colors.serialize_to_slice(buf))
                        .expect("Trying to serialize new data but DMA transfer is not finished");
                });

                 spi_tx.start()
                     .expect("If we were able to serialize we must be able to start!");
                 dbg.set_rx(true);
            });
        });
    }


    #[task(priority = 1, shared = [serial_rx], local = [stats: Option<ioqueue::Stats> = None])]
    fn debug_report(mut cx: debug_report::Context) {
        let old = cx.local.stats.get_or_insert_with(|| Default::default());
        let new = cx.shared.serial_rx.lock(|rx| {
            rx.stats().clone()
        });
        if &new != old {
            defmt::warn!("RX stats: {}", new);
            *old = new;
        }
    }

    #[task(binds = DMA1_CH4_5_6_7, priority = 4, shared = [spi_tx, dbg])]
    fn dma_spi_callback(mut cx: dma_spi_callback::Context) {
        cx.shared.spi_tx.lock(|spi_tx| {
           spi_tx.on_interrupt()
               .as_option()
               .transpose()
               .expect("Unexpected interrupt");
        });
        cx.shared.dbg.lock(|d| d.set_rx(false));
    }

    #[task(binds = DMA1_CH2_3, priority = 4, shared = [crc, serial_tx, serial_rx])]
    fn dma_uart_callback(cx: dma_uart_callback::Context) {
        let tx = cx.shared.serial_tx;
        let rx = cx.shared.serial_rx;
        let crc = cx.shared.crc;
        (tx, rx, crc).lock(|tx, rx, mut crc| {
            let rx_done = rx.on_interrupt(&mut crc)
                .as_option().transpose().expect("Unexpected interrupt");
            let tx_done = tx.on_interrupt()
                .as_option().transpose().expect("Unexpected interrupt");

            if rx_done.is_some() {
                defmt::debug!("UART RX done");
            }

            if tx_done.is_some() {
                defmt::debug!("UART TX done");
            }

            rx_done.or(tx_done).expect("No interrupt handled!");
        });
    }

    #[task(binds = USART1, priority = 4, shared = [crc, serial_rx], local = [
           empty_count: usize = 0,
    ])]
    fn uart_interrupt(cx: uart_interrupt::Context) {
        let rx = cx.shared.serial_rx;
        let crc = cx.shared.crc;
        (rx, crc).lock(|rx, mut crc| {
            rx.on_interrupt(&mut crc)
                .as_option().transpose().expect("Unexpected interrupt");
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
