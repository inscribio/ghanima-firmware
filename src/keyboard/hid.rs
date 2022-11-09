use bitfield::bitfield;
use keyberon::key_code::KeyCode;
use serde::{Serialize, Deserialize};
use ringbuffer::{ConstGenericRingBuffer, RingBufferWrite, RingBufferExt, RingBufferRead, RingBuffer};
use usb_device::{UsbError, class_prelude::*};
use usbd_hid::{hid_class::{HIDClass, ReportType}, descriptor::generator_prelude::*};

bitfield! {
    /// State of HID keyboard LEDs
    #[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
    pub struct KeyboardLeds(u8);
    pub num_lock, set_num_lock: 0;
    pub caps_lock, set_caps_lock: 1;
    pub scroll_lock, set_scroll_lock: 2;
    pub compose, set_compose: 3;
    pub kana, set_kana: 4;
}

/// Keyboard report compatible with Boot Keyboard
///
/// A standard HID report compatible with Boot Keyboard (see HID specification, Appendix B).
/// It can handle all modifier keys and up to 6 keys pressed at the same time.
#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = KEYBOARD) = {
        (usage_page = KEYBOARD, usage_min = 0xe0, usage_max = 0xe7) = {
            #[packed_bits 8] #[item_settings data,variable,absolute] modifier = input;
        };
        (usage_min = 0x00, usage_max = 0xff) = {
            #[item_settings constant,variable,absolute] reserved=input;
        };
        (usage_page = LEDS, usage_min = 0x01, usage_max = 0x05) = {
            #[packed_bits 5] #[item_settings data,variable,absolute] leds = output;
        };
        // It would make sense to use usage_max=0xdd but boot keyboard uses 0xff. This way
        // keycodes >= KeyCode::LCtrl (notably - "unofficial media") should still work
        // (though these only work on linux, we should use different usage page for media).
        (usage_page = KEYBOARD, usage_min = 0x00, usage_max = 0xff) = {
            #[item_settings data,array,absolute] keycodes = input;
        };
    }
)]
#[derive(Default, Eq, PartialEq)]
pub struct KeyboardReport {
    /// Modifier keys packed bits
    pub modifier: u8,
    /// Boot keyboard reserved field
    pub reserved: u8,
    /// LED states (host -> device)
    pub leds: u8,
    /// Boot keyboard keycodes list
    pub keycodes: [u8; 6],
}

pub struct HidKeyboard<'a, B: UsbBus> {
    hid: HIDClass<'a, B>,
    leds: KeyboardLeds,
}

impl<'a, B: UsbBus> HidKeyboard<'a, B> {
    pub fn new(alloc: &'a UsbBusAllocator<B>) -> Self {
        Self {
            hid: HIDClass::new_ep_in_with_settings(alloc, KeyboardReport::desc(), 10, Self::settings()),
            leds: KeyboardLeds(0),
        }
    }

    /// Get underlying USB class to be passed to poll()
    pub fn class(&mut self) -> &mut dyn UsbClass<B> {
        &mut self.hid
    }

    /// Push keyboard report to endpoint
    pub fn push_keyboard_report(&mut self, report: &KeyboardReport) -> usb_device::Result<usize> {
        self.hid.push_input(report)
    }

    /// Get current state of keyboard LEDs additionally returning true state changed since last read
    pub fn leds(&mut self) -> (KeyboardLeds, bool) {
        let mut changed = false;
        let mut data = 0u8;
        if let Ok(info) = self.hid.pull_raw_report(core::slice::from_mut(&mut data)) {
            if let ReportType::Output = info.report_type {
                if info.report_id == 0 && info.len == 1 {
                    if self.leds.0 != data {
                        self.leds.0 = data;
                        changed = true;
                    }
                }
            }
        }
        (self.leds, changed)
    }

    const fn settings() -> usbd_hid::hid_class::HidClassSettings {
        use usbd_hid::hid_class::*;
        HidClassSettings {
            subclass: HidSubClass::Boot,
            protocol: HidProtocol::Keyboard,
            config: ProtocolModeConfig::ForceBoot,
            locale: HidCountryCode::NotSupported,
        }
    }
}

impl core::iter::FromIterator<KeyCode> for KeyboardReport {
    fn from_iter<T>(iter: T) -> Self
where
        T: IntoIterator<Item = KeyCode>,
    {
        let mut res = Self::default();
        for kc in iter {
            res.pressed(kc);
        }
        res
    }
}

impl KeyboardReport {
    /// Add the given key code to the report. If the report is full,
    /// it will be set to `ErrorRollOver`.
    pub fn pressed(&mut self, kc: KeyCode) {
        use KeyCode::*;
        match kc {
            No => (),
            ErrorRollOver | PostFail | ErrorUndefined => self.set_all(kc),
            kc if kc.is_modifier() => self.modifier |= kc.as_modifier_bit(),
            _ => self.keycodes
                .iter_mut()
                .find(|c| **c == 0)
                .map(|c| *c = kc as u8)
                .unwrap_or_else(|| self.set_all(ErrorRollOver)),
        }
    }

    fn set_all(&mut self, kc: KeyCode) {
        for c in &mut self.keycodes.iter_mut() {
            *c = kc as u8;
        }
    }
}

/// Helper queue for sending USB HID reports
///
/// Due to unpredictable host OS polling it may happen that mcu generates
/// HID reports faster than OS is consumes them. This most often happens
/// in spikes, so adding a small FIFO queue in between allows to minimize
/// number of missed reports.
pub struct HidReportQueue<R, const N: usize> {
    queue: ConstGenericRingBuffer<R, N>,
    missed: bool,
}

impl<R, const N: usize> HidReportQueue<R, N>
    where R: PartialEq
{
    pub fn new() -> Self {
        Self {
            queue: ConstGenericRingBuffer::new(),
            missed: false,
        }
    }

    /// Push a report to queue if it changed
    ///
    /// Adds a new report to queue if it is different from the last one.
    pub fn push(&mut self, report: R) {
        // TODO: instead of having large queue, use smarter way of merging following keyboard reports
        // e.g. when pressing 4 keys, instead of inserting [A], [A, B], [A, B, C], [A, B, C, D], we
        // would first insert [A], then update that report; similarly when releasing:
        // [A, B, C, D], [A, B, C], [A, B], [A]
        // would be merged into all-to-nothing. But we must make sure that we don't accidentally miss
        // something when merging.
        // Define trait Report that would optionally provide merge(other).

        // Add new report only if it is different than the previous one or queue is empty.
        let add = self.queue.back()
            .map(|prev| &report != prev)
            .unwrap_or(true);

        // If we previously missed a report (ring buffer overflow) then we must make sure
        // that an additional report will be sent to synchronize the final HID state.
        if add || (self.missed && self.queue.is_empty()) { // TODO: could be `!is_full`?
            // If running out of queue space (host polling so rarely) then we just overwrite
            // first report in queue. As long as main loop is always pushing the current
            // report, we should be fine if we ensure to at least 1 report with non-full
            // queue (this means that reports are not changing now).
            self.missed = self.queue.is_full();
            self.queue.push(report);
        }
    }

    /// Try sending USB HID report
    ///
    /// This will try to send next report from queue assuming `write_report` is successful
    /// if it returns `OK(n)` with `n > 0`, which corresponds to standard endpoint write
    /// function. If it returns `Ok(0)` or `Err(UsbError::WouldBlock)` then we try later.
    ///
    /// # Panics
    ///
    /// When `write_report` returns `Err` other than `UsbError::WouldBlock`, which means
    /// there is a bug in class implementation.
    pub fn send<F>(&mut self, write_report: F)
        where F: FnOnce(&R) -> Result<usize, UsbError>
    {
        if let Some(report) = self.queue.peek() {
            // Call to .write() will return Ok(0) if the previous report hasn't been sent yet,
            // else number of data written. Any other Err should never happen - would be
            // BufferOverflow or error from UsbBus implementation (like e.g. InvalidEndpoint).
            let ok = write_report(report)
                .or_else(|e| match e {
                    UsbError::WouldBlock => Ok(0),
                    e => Err(e),
                })
                .map_err(|_| ())
                .expect("Bug in class implementation") > 0;
            if ok {
                // Consume the report on success
                self.queue.skip();
            }
        }
    }
}
