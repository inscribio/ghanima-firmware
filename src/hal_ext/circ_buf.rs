use core::cmp::Ordering;

use embedded_dma::WriteBuffer;

/// DMA RX circular buffer
///
/// Circular buffer designed to use with DMA. User code can only
/// consume data, data must be written by DMA.
pub(crate) struct CircularBuffer<BUF> {
    buf: BUF,
    head: u16,
    wrap_count: u8,
}

impl<BUF> CircularBuffer<BUF>
where
    BUF: WriteBuffer<Word = u8>,
{
    pub fn new(buf: BUF) -> Self {
        Self { buf, head: 0, wrap_count: 0 }
    }

    /// Get valid data from the buffer
    ///
    /// This will return two slices of data from the buffer because the memory
    /// may not be continues (buffer might have wrapped). Will also return the
    /// number of data that has been overwritten by DMA.
    pub fn consume(&mut self, tail: u16) -> (&[u8], &[u8], usize) {
        let buf = unsafe { self.buf() };
        let data = Self::get_data(buf, self.wrap_count, self.head, tail);

        // Mark as consumed
        self.head = tail;
        self.wrap_count = 0;

        data
    }

    fn get_data(buf: &[u8], wrap_count: u8, head: u16, tail: u16) -> (&[u8], &[u8], usize) {
        let (h, t) = (head as usize, tail as usize);
        let nil = &[][..];

        use Ordering::Equal as HeadOnTail;
        use Ordering::Less as HeadBeforeTail;
        use Ordering::Greater as TailBeforeHead;

        let (slice1, slice2, overwritten) = match (wrap_count, head.cmp(&tail)) {
            // No wrapping
            (0, HeadOnTail)     => (nil, nil, 0),  // no data
            (0, HeadBeforeTail) => (&buf[h..t], nil, 0),  // data [H, T)
            (0, TailBeforeHead) => unreachable!(),
            // Wrapped once
            (1, HeadOnTail)     => (&buf[h..], &buf[0..t], 0),  // whole buffer, DMA will overflow on next byte
            (1, TailBeforeHead) => (&buf[h..], &buf[0..t], 0),  // data [H, END) + [0, T)
            (1, HeadBeforeTail) => (&buf[t..], &buf[0..t], t - h),  // DMA has overwritten [H, T)
            // Wrapped twice or more ...
            (n, _) => (
                &buf[t..], &buf[0..t],
                // overwritten [H, END) + [0, T) + (N - 2) times whole buffer
                (buf.len() - h) + t + (n - 2) as usize * buf.len()
            ),
        };

        (slice1, slice2, overwritten)
    }

    #[allow(dead_code)]
    pub fn capacity_remaining(&mut self, tail: u16) -> usize {
        let buf = unsafe { self.buf() };
        let (s1, s2, _) = Self::get_data(buf, self.wrap_count, self.head, tail);
        buf.len() - (s1.len() + s2.len())
    }

    pub fn tail_wrapped(&mut self) {
        self.wrap_count += 1;
    }

    unsafe fn buf(&mut self) -> &'static [u8] {
        let (buf, len) = self.buf.write_buffer();
        core::slice::from_raw_parts(buf, len)
    }

    // DMA transfer mock
    #[cfg(test)]
    fn advance_dma(&mut self, tail: &mut u16, data: &[u8]) -> (&[u8], &[u8], usize) {
        let buf = unsafe {
            let (buf, len) = self.buf.write_buffer();
            core::slice::from_raw_parts_mut(buf, len)
        };
        // no need to be efficient in test cases
        for v in data {
            buf[*tail as usize] = *v;
            *tail += 1;
            // DMA NDTR is reset, so tail wraps to 0
            if *tail == buf.len() as u16 {
                *tail = 0;
                self.tail_wrapped();
            }
        }
        self.consume(*tail)
    }
}

// Defer WriteBuffer to the internal buffer
unsafe impl<BUF> WriteBuffer for CircularBuffer<BUF>
where
    BUF: WriteBuffer<Word = u8>
{
    type Word = <BUF as WriteBuffer>::Word;

    unsafe fn write_buffer(&mut self) -> (*mut Self::Word, usize) {
        self.buf.write_buffer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Assume that given buffer has static lifetime to satisfy DMA buffer constraints.
    // This is safe in the context of these unit tests, as we don't use DMA, so we only
    // use the buffer for the lifetime of a single test case.
    fn as_static(buf: &mut [u8]) -> &'static mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len())
        }
    }

    // Test case is a sequence of:
    // [dma_rx_data] => [consume_out_1], [consume_out_2], head = head_value_after_consuming;
    macro_rules! test_circ_buf {
        (
            LEN = $len:literal;
            $(
                [$($dma_input:literal),*] => ( [$($consume1:literal),*], [$($consume2:literal),*] ),
                head = $head:literal, lost = $lost:literal
            );+ $(;)?
        ) => {
            {
                let mut buf = [0xff; $len];
                let mut buf = CircularBuffer::new(as_static(&mut buf[..]));
                let mut tail = 0;
                $(
                    let input = [$($dma_input),*];
                    let o1 = [$($consume1),*];
                    let o2 = [$($consume2),*];
                    assert_eq!(buf.advance_dma(&mut tail, &input), (&o1[..], &o2[..], $lost));
                    assert_eq!(buf.head, $head);
                )+
            }
        };
    }

    #[test]
    fn empty() {
        test_circ_buf! {
            LEN = 7;
            [] => ([], []), head = 0, lost = 0;
        }
    }


    #[test]
    fn single_slice() {
        test_circ_buf! {
            LEN = 7;
            [1, 2, 3] => ([1, 2, 3], []), head = 3, lost = 0;
            [] => ([], []), head = 3, lost = 0;
            [4, 5] => ([4, 5], []), head = 5, lost = 0;
        }
    }

    #[test]
    fn whole_buf() {
        test_circ_buf! {
            LEN = 7;
            [1, 2, 3, 4, 5, 6, 7] => ([1, 2, 3, 4, 5, 6, 7], []), head = 0, lost = 0;
            [8, 9] => ([8, 9], []), head = 2, lost = 0;
            [10, 11, 12, 13, 14, 15, 16] => ([10, 11, 12, 13, 14], [15, 16]), head = 2, lost = 0;
        }
    }

    #[test]
    fn with_wrap() {
        test_circ_buf! {
            LEN = 7;
            [1, 2, 3, 4, 5] => ([1, 2, 3, 4, 5], []), head = 5, lost = 0;
            [6, 7, 8, 9, 10] => ([6, 7], [8, 9, 10]), head = 3, lost = 0;
        }
    }

    #[test]
    fn with_overflow() {
        test_circ_buf! {
            LEN = 7;
            [
                1, 2, 3, 4, 5, 6, 7,
                8, 9, 10
            ] => ([4, 5, 6, 7], [8, 9, 10]), head = 3, lost = 3;
            [
                /*           */ 11, 12, 13,
                14, 15, 16, 17, 18
            ] => ([12, 13, 14], [15, 16, 17, 18]), head = 4, lost = 1;
        }
    }

    #[test]
    fn double_wrap() {
        test_circ_buf! {
            LEN = 4;
            [
                1, 2, 3, 4,
                5, 6, 7, 8,
                9
            ] => ([6, 7, 8], [9]), head = 1, lost = 5;
            [
                /**/10, 11, 12,
                13, 14, 15, 16,
                17, 18
            ] => ([15, 16], [17, 18]), head = 2, lost = 5;
            [
                /*    */19, 20,
                21, 22, 23, 24,
                25
            ] => ([22, 23, 24], [25]), head = 1, lost = 3;
        }
    }

    #[test]
    fn multi_wrap() {
        test_circ_buf! {
            LEN = 4;
            [
                1_, 2_, 3_, 4_,
                5_, 6_, 7_, 8_,
                9_, 10, 11, 12,
                13, 14, 15, 16,
                17, 18, 19
            ] => ([16], [17, 18, 19]), head = 3, lost = 15;
            [
                /*       */ 20,
                21, 22, 23, 24,
                21, 22, 23, 24,
                21, 22, 23, 24
            ] => ([21, 22, 23, 24], []), head = 0, lost = 9;
        }
    }
}
