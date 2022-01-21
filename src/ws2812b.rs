use static_assertions as sa;
use rgb::{RGB8, ComponentSlice};

// SPI frequency: 3 MHz; Bit time: 333 ns
const SPI_FREQ: usize = 3_000_000;
const T0H_BITS: usize = 1;  // 333 ns (vs 220-380 ns)
const T0L_BITS: usize = 3;  // 1000 ns (vs 580-1000 ns)
const T1H_BITS: usize = 2;  // 666 ns (vs 580-1000 ns)
const T1L_BITS: usize = 2;  // 666 ns (vs 580-1000 ns)
const RESET_US: usize = 280;

// Currently assuming we use the same bit count for 0 and 1.
// This allows to index buffer with serialized data.
sa::const_assert_eq!(T0L_BITS + T0H_BITS, T1L_BITS + T1H_BITS);
const SERIAL_BITS: usize = T0L_BITS + T0H_BITS;

// Data for each LED with 3x8=24-bit RGB color, with each bit serialized as X bits.
const LEDS_COUNT: usize = 28;
const RGB_BITS: usize = 3 * 8;
const fn led_bits(leds_count: usize) -> usize {
    leds_count * RGB_BITS * SERIAL_BITS
}

// FIXME: change both to false when line idle state is fixed in hardware
const RESET_BEFORE: bool = true;
const RESET_AFTER: bool = true;
const RESET_BITS: usize = RESET_US * (SPI_FREQ / 1_000_000);
const RESET_BITS_BEFORE: usize = if RESET_BEFORE { RESET_BITS } else { 0 };
const RESET_BITS_AFTER: usize = if RESET_AFTER { RESET_BITS } else { 0 };

const fn all_bits(leds_count: usize) -> usize {
    RESET_BITS_BEFORE + led_bits(leds_count) + RESET_BITS_AFTER
}

const fn bytes_for_bits(bits: usize) -> usize {
    (bits + 7) / 8
}

pub const BUFFER_SIZE: usize = bytes_for_bits(all_bits(LEDS_COUNT));
pub type Buffer = [u8; BUFFER_SIZE];
pub const BUFFER_ZERO: Buffer = [0u8; BUFFER_SIZE];
const SERIAL_SIZE: usize = bytes_for_bits(SERIAL_BITS);

pub struct Leds {
    pub leds: [RGB8; LEDS_COUNT],
}

impl Leds {
    pub const fn new() -> Self {
        Self {
            leds: [RGB8::new(0, 0, 0); LEDS_COUNT],
        }
    }

    const fn serial_bits(high_bits: usize) -> [u8; SERIAL_SIZE] {
        let mut arr = [0; SERIAL_SIZE];
        let mut i = 0;
        while i < high_bits {
            let bit = 7 - i % 8;  // msb first
            arr[i / 8] |= 1 << bit;
            i += 1;
        }
        arr
    }

    const ONE: [u8; SERIAL_SIZE] = Self::serial_bits(T1H_BITS);
    const ZERO: [u8; SERIAL_SIZE] = Self::serial_bits(T0H_BITS);

    const fn serial_mask(bit_value: bool, first_half: bool) -> u8 {
        // This is a specialized implementation
        sa::const_assert_eq!(SERIAL_BITS, 4);
        match (bit_value, first_half) {
            (false, true)  => Self::ZERO[0],
            (false, false) => Self::ZERO[0] >> 4,
            (true,  true)  => Self::ONE[0],
            (true,  false) => Self::ONE[0] >> 4,
        }
    }

    fn serialize_colors(colors: &[RGB8], buf: &mut [u8]) {
        let mut i = 0;
        let bit_msb = |byte: u8, i: usize| (byte & (1 << (7 - i))) != 0;
        for rgb in colors {
            let mut serialize_byte = |c: u8| {
                for j in 0..4 {
                    let n1 = Self::serial_mask(bit_msb(c, 2*j), true);
                    let n2 = Self::serial_mask(bit_msb(c, 2*j + 1), false);
                    buf[i] = n1 | n2;
                    i += 1;
                }
            };
            serialize_byte(rgb.g);
            serialize_byte(rgb.r);
            serialize_byte(rgb.b);
        }
    }

    // Serialize all RGB values to given buffer
    pub fn serialize(&mut self, buf: &mut [u8; BUFFER_SIZE]) {
        self.serialize_to_slice(&mut buf[..])
    }

    // Serialize all RGB values to given buffer
    //
    // # Panics
    //
    // If the buffer is not large enough - it must be at least BUFFER_SIZE bytes.
    pub fn serialize_to_slice(&mut self, buf: &mut [u8]) {
        let data = &mut buf[RESET_BITS_BEFORE/8..(RESET_BITS_BEFORE+led_bits(self.leds.len()))/8];
        Self::serialize_colors(&self.leds, data);
    }
}

#[cfg(test)]
mod tests {
    use std::println;

    use super::*;

    #[test]
    fn const_led_bits() {
        assert_eq!(led_bits(LEDS_COUNT), 2688);
    }

    #[test]
    fn const_all_bits() {
        let reset_bits = match (RESET_BEFORE, RESET_AFTER) {
            (true, true) => 1680,
            (false, true) | (true, false) => 840,
            (false, false) => 0,
        };
        assert_eq!(all_bits(LEDS_COUNT), 2688 + reset_bits);
    }

    #[test]
    fn const_buf_size() {
        let bytes = match (RESET_BEFORE, RESET_AFTER) {
            (true, true) => 546,  // bits: 1680 + 2688 = 4368
            (false, true) | (true, false) => 441,  // bits: 840 + 2688 = 3528
            (false, false) => 336,  // bits: 2688
        };
        assert_eq!(BUFFER_SIZE, bytes);
    }

    #[test]
    fn serialize_one() {
        let leds = [RGB8::new(0xff, 0xaa, 0x31)];
        let mut buf = [0u8; 3 * 8 / 2];
        Leds::serialize_colors(&leds, &mut buf);
        let expected = [
            // green: 0xaa = 0b10101010
            0b1100_1000, 0b1100_1000, 0b1100_1000, 0b1100_1000,
            // red: 0xff = 0b11111111
            0b1100_1100, 0b1100_1100, 0b1100_1100, 0b1100_1100,
            // blue: 0x31 = 0b00110001
            0b1000_1000, 0b1100_1100, 0b1000_1000, 0b1000_1100,
        ];
        assert_eq!(buf, expected, "\n  {:02x?}\n  vs\n  {:02x?}\n", buf, expected);
    }

    #[test]
    fn serialize_multiple() {
        let leds = [RGB8::new(0xff, 0xaa, 0x31), RGB8::new(0xaa, 0x31, 0xff)];
        let mut buf = [0u8; (3 * 8 / 2) * 2];
        Leds::serialize_colors(&leds, &mut buf);
        let expected = [
            0b1100_1000, 0b1100_1000, 0b1100_1000, 0b1100_1000, // 0xaa
            0b1100_1100, 0b1100_1100, 0b1100_1100, 0b1100_1100, // 0xff
            0b1000_1000, 0b1100_1100, 0b1000_1000, 0b1000_1100, // 0x31
            0b1000_1000, 0b1100_1100, 0b1000_1000, 0b1000_1100, // 0x31
            0b1100_1000, 0b1100_1000, 0b1100_1000, 0b1100_1000, // 0xaa
            0b1100_1100, 0b1100_1100, 0b1100_1100, 0b1100_1100, // 0xff
        ];
        assert_eq!(buf, expected, "\n  {:02x?}\n  vs\n  {:02x?}\n", buf, expected);
    }
}
