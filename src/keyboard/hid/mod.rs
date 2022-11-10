mod consumer;
mod keyboard;
mod mouse;

use ringbuffer::{ConstGenericRingBuffer, RingBufferWrite, RingBufferExt, RingBufferRead, RingBuffer};
use usb_device::{UsbError, class_prelude::*};
use usbd_hid::descriptor::AsInputReport;

pub use keyboard::{HidKeyboard, KeyboardReport, KeyboardLeds};
pub use mouse::{HidMouse, MouseReport};
pub use consumer::{HidConsumer, ConsumerReport, ConsumerKey};

/// Specific HID class
pub trait HidClass<'a, B: UsbBus + 'a> {
    type Report: AsInputReport;

    /// Get underlying USB HID class
    fn class(&mut self) -> &mut usbd_hid::hid_class::HIDClass<'a, B>;

    /// Push report to endpoint
    fn push_report(&mut self, report: &Self::Report) -> usb_device::Result<usize> {
        self.class().push_input(report)
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
    where R: AsInputReport + PartialEq
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
    pub fn send<'a, C, B>(&mut self, hid: &mut C)
        where
            B: UsbBus + 'a,
            C: HidClass<'a, B, Report = R>,
    {
        if let Some(report) = self.queue.peek() {
            // Call to .write() will return Ok(0) if the previous report hasn't been sent yet,
            // else number of data written. Any other Err should never happen - would be
            // BufferOverflow or error from UsbBus implementation (like e.g. InvalidEndpoint).
            let ok = hid.class().push_input(report)
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

    /// Emtpy the report queue, to be called on USB disconnect/suspend
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}
