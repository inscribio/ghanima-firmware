use static_assertions as sa;
use rgb::RGB8;

/// Assumed SPI frequency: 3 MHz; Bit time: 333 ns
pub const SPI_FREQ: usize = 3_000_000;
const T0H_BITS: usize = 1;  // 333 ns (vs 220-380 ns)
const T0L_BITS: usize = 3;  // 1000 ns (vs 580-1000 ns)
const T1H_BITS: usize = 2;  // 666 ns (vs 580-1000 ns)
#[allow(dead_code)]
const T1L_BITS: usize = 2;  // 666 ns (vs 580-1000 ns)
const RESET_US: usize = 280;

// Currently assuming we use the same bit count for 0 and 1.
// This allows to index buffer with serialized data.
sa::const_assert_eq!(T0L_BITS + T0H_BITS, T1L_BITS + T1H_BITS);
const SERIAL_BITS: usize = T0L_BITS + T0H_BITS;

// Data for each LED with 3x8=24-bit RGB color, with each bit serialized as X bits.
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

const SERIAL_SIZE: usize = bytes_for_bits(SERIAL_BITS);

/// Structure holding RGB LED colors for the whole board
///
/// Provides methods to serialize RGB data into format suitable for transmission
/// via SPI configured with frequency of [`SPI_FREQ`].
pub struct Leds<const N: usize> {
    pub leds: [RGB8; N],
}

impl<const N: usize> Leds<N> {
    /// Size of buffer needed for serialized LED data
    pub const BUFFER_SIZE: usize = bytes_for_bits(all_bits(N));
    // /// Zero-initialized buffer for serialized data
    // pub const fn buffer_zeroed() -> [u8; Leds::<{ N }>::BUFFER_SIZE] {
    //     [0u8; Leds::BUFFER_SIZE]
    // }

    /// Intialize with all LEDs diabled (black)
    pub const fn new() -> Self {
        Self {
            leds: [RGB8::new(0, 0, 0); N],
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

    /// Serialize all RGB values to given buffer
    ///
    /// # Panics
    ///
    /// If the buffer is not large enough - it must be at least [`Self::BUFFER_SIZE`] bytes.
    pub fn serialize_to_slice(&mut self, buf: &mut [u8]) -> usize {
        let data = &mut buf[RESET_BITS_BEFORE/8..(RESET_BITS_BEFORE+led_bits(self.leds.len()))/8];
        Self::serialize_colors(&self.leds, data);
        Self::BUFFER_SIZE
    }

    const fn gamma_correction(pixel: u8) -> u8 {
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

    /// Set colors to a pattern suitable for testing LEDs
    pub fn set_test_pattern(&mut self, t: usize, brightness: u8) {
        let reflect = |v: usize| {
            let v = v % 512;
            if v >= 256 {
                511 - v
            } else {
                v
            }
        };
        let dimmed = |v: usize| {
            ((v * brightness as usize) / 256) as u8
        };
        for (i, led) in self.leds.iter_mut().enumerate() {
            led.r = Self::gamma_correction(dimmed(reflect(t/1 + 4*i)));
            led.g = Self::gamma_correction(dimmed(reflect(t/2 + 2*i)));
            led.b = Self::gamma_correction(dimmed(reflect(t/3 + 3*i)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_led_bits() {
        assert_eq!(led_bits(28), 2688);
    }

    #[test]
    fn const_all_bits() {
        let reset_bits = match (RESET_BEFORE, RESET_AFTER) {
            (true, true) => 1680,
            (false, true) | (true, false) => 840,
            (false, false) => 0,
        };
        assert_eq!(all_bits(28), 2688 + reset_bits);
    }

    #[test]
    fn const_buf_size() {
        let bytes = match (RESET_BEFORE, RESET_AFTER) {
            (true, true) => 546,  // bits: 1680 + 2688 = 4368
            (false, true) | (true, false) => 441,  // bits: 840 + 2688 = 3528
            (false, false) => 336,  // bits: 2688
        };
        assert_eq!(Leds::<28>::BUFFER_SIZE, bytes);
    }

    #[test]
    fn serialize_one() {
        let leds = [RGB8::new(0xff, 0xaa, 0x31)];
        let mut buf = [0u8; 3 * 8 / 2];
        Leds::<28>::serialize_colors(&leds, &mut buf);
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
        Leds::<28>::serialize_colors(&leds, &mut buf);
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
