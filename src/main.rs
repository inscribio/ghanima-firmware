#![no_main]
#![no_std]

use panic_probe as _;
use defmt_rtt as _;
use stm32f0xx_hal as hal;
use ghanima as lib;

#[rtic::app(device = crate::hal::pac, dispatchers = [CEC_CAN, USART3_4])]
mod app {
    use core::mem::MaybeUninit;
    use cortex_m::interrupt::free as ifree;
    use super::hal;
    use hal::prelude::*;
    use usb_device::class_prelude::UsbBusAllocator;

    use super::lib;
    use lib::bsp::{self, debug, joystick, ws2812b, usb::Usb, sides::BoardSide};
    use lib::hal_ext::{crc, spi, reboot, uart, watchdog, dma::{DmaSplit, DmaTx}};
    use lib::{keyboard, config, ioqueue};

    // MCU clock frequencies
    const SYSCLK_MHZ: u32 = 48;
    const PCLK_MHZ: u32 = 24;
    const CRYSTAL_CLK_MHZ: u32 = 12;

    /// Base frequency of a "tick"
    const TICK_FREQUENCY_HZ: u32 = 1000;
    // Prescalers that define task frequencies in multiples of a "tick"
    const KEYBOARD_PRESCALER: u32 = 1;
    const LEDS_PRESCALER: u32 = 10;
    const JOY_PRESCALER: u32 = 10;
    const DEBUG_PRESCALER: u32 = 1000;

    const ERROR_LED_DURATION_MS: u32 = 1000;
    const DEBOUNCE_COUNT: u16 = 5;

    const WATCHDOG_WINDOW_START_MS: u32 = 30;
    const WATCHDOG_WINDOW_END_MS: u32 = 60;

    type SerialTx = keyboard::Transmitter<uart::Tx, 4>;
    type SerialRx = keyboard::Receiver<uart::Rx<&'static mut [u8]>, 4, 32>;
    type Leds = ws2812b::Leds<{ bsp::NLEDS }>;
    type Keyboard = keyboard::Keyboard<{ config::N_LAYERS }>;

    // Using &'static mut to avoid unnecessary stack allocations, see:
    // https://github.com/rtic-rs/cortex-m-rtic/blob/master/examples/big-struct-opt.rs
    #[shared]
    struct Shared {
        usb: &'static mut Usb,
        spi_tx: spi::SpiTx,
        serial_tx: SerialTx,
        serial_rx: SerialRx,
        crc: crc::Crc,
        led_controller: &'static mut keyboard::LedController<'static>,
        led_output: keyboard::LedOutput,
        keyboard: &'static mut Keyboard,
        // Task counters
        tick_cnt: debug::TaskCounter,
        usb_cnt: debug::TaskCounter,
        keyboard_cnt: debug::TaskCounter,
        joystick_cnt: debug::TaskCounter,
        leds_update_cnt: debug::TaskCounter,
        leds_tick_cnt: debug::TaskCounter,
        dma_spi_cnt: debug::TaskCounter,
        dma_uart_cnt: debug::TaskCounter,
        uart_cnt: debug::TaskCounter,
        idle_cnt: debug::TaskCounter,
    }

    #[local]
    struct Local {
        timer: hal::timers::Timer<hal::pac::TIM15>,
        joy: joystick::Joystick,
        watchdog: watchdog::WindowWatchdog,
    }

    #[monotonic(binds = SysTick, default = true)]
    type Mono = systick_monotonic::Systick<MONO_HZ>;
    pub const MONO_HZ: u32 = 1000;

    // This runs before main, even before initialization of .bss and .data so be careful.
    #[cortex_m_rt::pre_init]
    unsafe fn pre_init() {
        reboot::maybe_jump_bootloader();
        if cfg!(feature = "stack-usage") {
            // Use some margin as it seems we're actually corrupting some "theoretically free" stack
            debug::mem::free_stack_fill(0x40);
        }
    }

    #[init(local = [
        usb: MaybeUninit<Usb> = MaybeUninit::uninit(),
        led_controller: MaybeUninit<keyboard::LedController<'static>> = MaybeUninit::uninit(),
        keyboard: MaybeUninit<keyboard::Keyboard<{ config::N_LAYERS }>> = MaybeUninit::uninit(),
        usb_bus: Option<UsbBusAllocator<hal::usb::UsbBusType>> = None,
        led_buf: [u8; Leds::BUFFER_SIZE] = [0; Leds::BUFFER_SIZE],
        serial_tx_buf: [u8; 64] = [0; 64],
        serial_rx_buf: [u8; 128] = [0; 128],
    ])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        let mut core = cx.core;
        let mut dev = cx.device;

        // Automatically enter sleep mode when leaving an ISR
        // Disable when watchdog is active, so that we always enter idle task to feed it.
        if cfg!(feature = "idle-sleep") && !cfg!(feature = "watchdog") {
            core.SCB.set_sleeponexit();
        }

        // Clock configuration (may use external crystal, but it is not needed for STM32F072)
        let clk_config = dev.RCC
            .configure()
            .enable_crs(dev.CRS) // synchronization to USB SOF
            .sysclk(SYSCLK_MHZ.mhz())
            .pclk(PCLK_MHZ.mhz());
        let clk_config = if cfg!(feature = "crystal") {
            clk_config.hse(CRYSTAL_CLK_MHZ.mhz(), hal::rcc::HSEBypassMode::NotBypassed)
        } else {
            clk_config.hsi48()
        };
        let mut rcc = clk_config.freeze(&mut dev.FLASH);

        // Check if a watchdog reset occured, clear the flags
        let was_watchdog_reset = watchdog::reset_flags::was_window_watchdog(&mut rcc);
        watchdog::reset_flags::clear(&mut rcc);

        // Watchdog
        const PARAMS: watchdog::WindowParams = watchdog::WindowParams::new(
            PCLK_MHZ * 1_000_000,
            WATCHDOG_WINDOW_START_MS * 1000,
            WATCHDOG_WINDOW_END_MS * 1000,
        );
        let mut watchdog = watchdog::WindowWatchdog::new(dev.WWDG, PARAMS);
        if cfg!(feature = "watchdog") {
            watchdog.stop_on_debug(true, &mut dev.DBGMCU, &mut rcc);
            watchdog.start(&mut rcc);
        };

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
        debug::tasks::init(dev.USART2, (debug_tx, debug_rx), &mut rcc);

        // ADC
        let joy_x = ifree(|cs| gpioa.pa0.into_analog(cs));
        let joy_y = ifree(|cs| gpioa.pa1.into_analog(cs));
        let mut joy = joystick::Joystick::new(dev.ADC, (joy_y, joy_x), &mut rcc);

        // SPI (tx only) for RGB data
        // HAL provides only a blocking interface, so we must configure everything on our own
        let rgb_tx = ifree(|cs| gpiob.pb15.into_alternate_af0(cs));  // SPI2_MOSI
        let mut spi_tx = spi::SpiTx::new(dev.SPI2, rgb_tx, dma.ch5, &mut cx.local.led_buf[..], 3.mhz(), &mut rcc);

        // configure periodic timer
        let mut timer = hal::timers::Timer::tim15(dev.TIM15, TICK_FREQUENCY_HZ.hz(), &mut rcc);
        timer.listen(hal::timers::Event::TimeOut);

        // USB
        let usb = hal::usb::Peripheral {
            usb: dev.USB,
            pin_dp: gpioa.pa12,
            pin_dm: gpioa.pa11
        };
        *cx.local.usb_bus = Some(hal::usb::UsbBus::new(usb));
        let usb_bus = cx.local.usb_bus.as_ref().unwrap();

        let usb = unsafe {
            cx.local.usb.as_mut_ptr().write(Usb::new(usb_bus, &board_side, config::CONFIG.bootload_strict));
            &mut *cx.local.usb.as_mut_ptr()
        };

        // LED controller
        let mut led_output = keyboard::LedOutput::new();
        let led_controller = unsafe {
            cx.local.led_controller.as_mut_ptr().write(
                keyboard::LedController::new(board_side, &config::CONFIG.leds)
            );
            &mut *cx.local.led_controller.as_mut_ptr()
        };

        // Keyboard
        let serial_tx = keyboard::Transmitter::new(serial_tx);
        let serial_rx = keyboard::Receiver::new(serial_rx);
        let keys = keyboard::Keys::new(board_side, cols, rows, DEBOUNCE_COUNT);
        let keyboard = unsafe {
            cx.local.keyboard.as_mut_ptr().write(keyboard::Keyboard::new(keys, &config::CONFIG));
            &mut *cx.local.keyboard.as_mut_ptr()
        };

        // If there was abnormal reset, signalize it using LEDs
        if was_watchdog_reset {
            defmt::error!("Watchdog triggered system reset");
            let ticks = ERROR_LED_DURATION_MS * 1000 / TICK_FREQUENCY_HZ / KEYBOARD_PRESCALER;
            let leds = led_output.set_overwrite(ticks as u16);
            for (i, led) in leds.colors.iter_mut().enumerate() {
                led.r = if i % 4 == 0 { 255 } else { 0 };
                led.g = 0;
                led.b = 0;
            }
        }

        // Send a first transfer ASAP with all LEDs in initial state
        {
            let colors = led_output.tick(0, led_controller);
            spi_tx.push(|buf| colors.serialize_to_slice(buf)).map_err(drop).unwrap();
            spi_tx.start().map_err(drop).unwrap();
        }

        if !joy.detect() {
            defmt::warn!("Joystick not detected");
        }

        let mono = systick_monotonic::Systick::new(core.SYST, rcc.clocks.sysclk().0);

        // Configure timestamps logging, u32 is ~50 days.
        defmt::timestamp!("[{=u32:06}]", monotonics::now().ticks() as u32);

        debug::tasks::trace::run(|| defmt::info!("Liftoff!"));

        watchdog.maybe_feed();

        if cfg!(feature = "stack-usage") {
            debug::mem::print_stack_info();
        }

        let shared = Shared {
            usb,
            spi_tx,
            serial_tx,
            serial_rx,
            crc,
            led_controller,
            led_output,
            keyboard,
            // Task counters
            tick_cnt: Default::default(),
            usb_cnt: Default::default(),
            keyboard_cnt: Default::default(),
            joystick_cnt: Default::default(),
            leds_update_cnt: Default::default(),
            leds_tick_cnt: Default::default(),
            dma_spi_cnt: Default::default(),
            dma_uart_cnt: Default::default(),
            uart_cnt: Default::default(),
            idle_cnt: Default::default(),
        };

        let local = Local {
            timer,
            joy,
            watchdog,
        };

        (shared, local, init::Monotonics(mono))
    }

    #[task(binds = TIM15, priority = 4, local = [timer, t: u32 = 0], shared = [tick_cnt])]
    fn tick(mut cx: tick::Context) {
        debug::tasks::task::enter(b't');
        cx.shared.tick_cnt.lock(|cnt| cnt.inc());

        // Clears interrupt flag
        if cx.local.timer.wait().is_ok() {
            // Spawn periodic tasks. Ignore error if we're too slow. Don't always compare
            // to 0 to avoid situations that all tasks are being run at the same tick.
            let t = cx.local.t;
            *t += 1;

            if *t % KEYBOARD_PRESCALER == 0 {
                if keyboard_tick::spawn(*t).is_err() {
                    defmt::error!("Spawn failed: keyboard_tick");
                }
            }

            if *t % LEDS_PRESCALER == 1 {
                if leds_tick::spawn(*t).is_err() {
                    defmt::warn!("Spawn failed: leds_tick");
                };
            }

            if *t % JOY_PRESCALER == 2 {
                if read_joystick::spawn().is_err() {
                    defmt::warn!("Spawn failed: read_joystick");
                };
            }

            if *t % DEBUG_PRESCALER == 3 {
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
    #[task(binds = USB, priority = 3, shared = [usb, usb_cnt])]
    fn usb_poll(mut cx: usb_poll::Context) {
        debug::tasks::task::enter(b'U');
        cx.shared.usb_cnt.lock(|cnt| cnt.inc());

        cx.shared.usb.lock(|usb| {
            // UsbDevice.poll()->UsbBus.poll() inspects and clears USB interrupt flags.
            // If there was data packet to any class this will return true.
            let _was_packet = usb.poll();
        });

        debug::tasks::task::exit();
    }

    #[task(
        priority = 2, capacity = 1,
        shared = [serial_tx, serial_rx, crc, usb, keyboard, keyboard_cnt],
        local = [prev_leds_update: Option<keyboard::LedsUpdate> = None],
    )]
    fn keyboard_tick(cx: keyboard_tick::Context, t: u32) {
        debug::tasks::task::enter(b'k');
        let keyboard_tick::SharedResources {
            serial_tx: mut tx,
            serial_rx: rx,
            crc,
            mut usb,
            mut keyboard,
            mut keyboard_cnt,
        } = cx.shared;
        keyboard_cnt.lock(|cnt| cnt.inc());

        // Bootloader reboot may happen here
        usb.lock(|usb| usb.dfu.tick(KEYBOARD_PRESCALER.try_into().unwrap()));

        // Run main keyboard logic
        let leds_update = keyboard.lock(|keyboard| keyboard.tick((&mut tx, rx), usb));

        // Transmit any serial messages
        (tx, crc).lock(|tx, crc| tx.tick(crc));

        // Send LED patterns update for processing later
        if update_leds_state::spawn(t, leds_update).is_err() {
            defmt::warn!("Spawn failed: update_leds_state");
        }

        debug::tasks::task::exit();
    }

    #[task(priority = 1, shared = [keyboard, joystick_cnt], local = [joy, certainty: u8 = 0])]
    fn read_joystick(mut cx: read_joystick::Context) {
        debug::tasks::task::enter(b'j');
        cx.shared.joystick_cnt.lock(|cnt| cnt.inc());

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

        debug::tasks::task::exit();
    }

    /// Apply state updates from keyboard_tick
    ///
    /// This has the same priority as update_leds but we use a queue to eventually apply all
    /// the updates.
    #[task(priority = 1, shared = [led_controller, led_output, leds_update_cnt], capacity = 4)]
    fn update_leds_state(mut cx: update_leds_state::Context, t: u32, update: keyboard::LedsUpdate) {
        debug::tasks::task::enter(b's');
        cx.shared.leds_update_cnt.lock(|cnt| cnt.inc());

        cx.shared.led_controller.lock(|ledctl| update.apply(t, ledctl));
        cx.shared.led_output.lock(|out| out.use_from_controller());

        debug::tasks::task::exit();
    }

    #[task(priority = 1, shared = [spi_tx, led_controller, led_output, leds_tick_cnt])]
    fn leds_tick(cx: leds_tick::Context, t: u32) {
        debug::tasks::task::enter(b'l');
        let leds_tick::SharedResources {
            mut spi_tx,
            led_controller,
            mut led_output,
            mut leds_tick_cnt,
        } = cx.shared;
        leds_tick_cnt.lock(|cnt| cnt.inc());

        // Generate LED colors
        (&mut led_output, led_controller).lock(|out, ctl| {
            debug::tasks::trace::run(|| out.tick(t, ctl));
        });

        // Send in separate lock to decrease time when serial tx is locked
        led_output.lock(|out| {
            let colors = out.current();

            // Prepare data to be sent and start DMA transfer.
            // `leds` must be kept locked because we're serializing from reference.
            spi_tx.lock(|spi_tx| {
                // Fails on first call because we start an immediate transfer in init()
                let ok = debug::tasks::trace::run(|| {
                    // TODO: try to use .serialize()
                    spi_tx.push(|buf| colors.serialize_to_slice(buf)).is_ok()
                });

                if !ok {
                    defmt::warn!("Trying to serialize new data but DMA transfer is not finished");
                } else {
                    spi_tx.start()
                        .map_err(drop)
                        .expect("If we were able to serialize we must be able to start!");
                    debug::tasks::trace::start();
                }
            });
        });

        debug::tasks::task::exit();
    }


    #[task(
        priority = 1,
        shared = [
            serial_rx,
            tick_cnt,
            usb_cnt,
            keyboard_cnt,
            joystick_cnt,
            leds_update_cnt,
            leds_tick_cnt,
            dma_spi_cnt,
            dma_uart_cnt,
            uart_cnt,
            idle_cnt,
        ],
        local = [stats: Option<ioqueue::Stats> = None]
    )]
    fn debug_report(mut cx: debug_report::Context) {
        debug::tasks::task::enter(b'd');

        let old = cx.local.stats.get_or_insert_with(|| Default::default());
        let new = cx.shared.serial_rx.lock(|rx| {
            rx.stats().clone()
        });
        if &new != old {
            defmt::warn!("RX stats: {}", new);
            *old = new;
        }

        if cfg!(feature = "task-counters") {
            (
                cx.shared.tick_cnt,
                cx.shared.usb_cnt,
                cx.shared.keyboard_cnt,
                cx.shared.joystick_cnt,
                cx.shared.leds_update_cnt,
                cx.shared.leds_tick_cnt,
                cx.shared.dma_spi_cnt,
                cx.shared.dma_uart_cnt,
                cx.shared.uart_cnt,
                cx.shared.idle_cnt,
            ).lock(|tick, usb, keyboard, joystick, leds_update, leds_tick, dma_spi, dma_uart, uart, idle| {
                    defmt::info!("tick={=u16} usb={=u16} kbd={=u16} joy={=u16} ledsU={=u16} ledsT={=u16} dma_spi={=u16} dma_uart={=u16} uart={=u16} idle={=u16}",
                        tick.pop(), usb.pop(), keyboard.pop(), joystick.pop(), leds_update.pop(), leds_tick.pop(), dma_spi.pop(), dma_uart.pop(), uart.pop(), idle.pop(),
                    );
                });
        }

        if cfg!(feature = "stack-usage") {
            debug::mem::print_stack_info();
        }

        debug::tasks::task::exit();
    }

    #[task(binds = DMA1_CH4_5_6_7, priority = 4, shared = [spi_tx, dma_spi_cnt])]
    fn dma_spi_callback(mut cx: dma_spi_callback::Context) {
        debug::tasks::task::enter(b'A');
        cx.shared.dma_spi_cnt.lock(|cnt| cnt.inc());

        cx.shared.spi_tx.lock(|spi_tx| {
            spi_tx.on_interrupt()
                .as_option()
                .transpose()
                .expect("Unexpected interrupt");
        });

        debug::tasks::trace::end();
        debug::tasks::task::exit();
    }

    #[task(binds = DMA1_CH2_3, priority = 4, shared = [crc, serial_tx, serial_rx, dma_uart_cnt])]
    fn dma_uart_callback(mut cx: dma_uart_callback::Context) {
        debug::tasks::task::enter(b'B');
        cx.shared.dma_uart_cnt.lock(|cnt| cnt.inc());

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

    #[task(binds = USART1, priority = 4, shared = [crc, serial_rx, uart_cnt])]
    fn uart_interrupt(mut cx: uart_interrupt::Context) {
        debug::tasks::task::enter(b'u');
        cx.shared.uart_cnt.lock(|cnt| cnt.inc());

        let rx = cx.shared.serial_rx;
        let crc = cx.shared.crc;
        (rx, crc).lock(|rx, mut crc| {
            rx.on_interrupt(&mut crc)
                .as_option().transpose().expect("Unexpected interrupt");
        });

        debug::tasks::task::exit();
    }

    #[idle(local = [watchdog], shared = [idle_cnt])]
    fn idle(mut cx: idle::Context) -> ! {
        loop {
            cx.shared.idle_cnt.lock(|cnt| cnt.inc());
            cx.local.watchdog.maybe_feed();

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
