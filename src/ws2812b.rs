use static_assertions as sa;
use bitvec::{BitArr, order::Msb0, view::BitView, slice::BitSlice, bits, array::BitArray, bitarr};
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

pub const BUFFER_SIZE: usize = bitvec::mem::elts::<u8>(all_bits(LEDS_COUNT));
pub type Buffer = [u8; BUFFER_SIZE];
pub const BUFFER_ZERO: Buffer = [0u8; BUFFER_SIZE];
const SERIAL_SIZE: usize = bitvec::mem::elts::<u8>(SERIAL_BITS);

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

    fn serialize_colors(colors: &[RGB8], buf: &mut BitSlice<u8, Msb0>) {
        let mut chunks = unsafe {
            buf[RESET_BITS_BEFORE..RESET_BITS_BEFORE+led_bits(colors.len())]
                .chunks_exact_mut(SERIAL_BITS)
                .remove_alias()
        };

        for rgb in colors {
            let grb = [rgb.g, rgb.r, rgb.b];
            for bit in grb.view_bits::<Msb0>() {
                let chunk = chunks.next().unwrap();
                chunk.copy_from_bitslice(if *bit {
                    &Self::ONE.view_bits()[..SERIAL_BITS]
                } else {
                    &Self::ZERO.view_bits()[..SERIAL_BITS]
                });
            }
        }
    }

    // buf must be large enough (underlying buffer of BitBuffer)
    pub fn serialize(&mut self, buf: &mut [u8]) {
        Self::serialize_colors(&self.leds, buf.view_bits_mut())
    }
}

#[cfg(test)]
mod tests {
    use std::println;

    use super::*;

    #[test]
    fn serialize_one() {
        let leds = [RGB8::new(0xff, 0xaa, 0x31)];
        let mut buf = [0u8; 3 * 8 / 2];
        Leds::serialize_colors(&leds, buf.view_bits_mut::<Msb0>());
        assert_eq!(buf, [
            // green: 0xaa = 0b10101010
            0b1100_1000, 0b1100_1000, 0b1100_1000, 0b1100_1000,
            // red: 0xff = 0b11111111
            0b1100_1100, 0b1100_1100, 0b1100_1100, 0b1100_1100,
            // blue: 0x31 = 0b00110001
            0b1000_1000, 0b1100_1100, 0b1000_1000, 0b1000_1100,
        ]);
    }

    #[test]
    fn serialize_multiple() {
        let leds = [RGB8::new(0xff, 0xaa, 0x31), RGB8::new(0xaa, 0x31, 0xff)];
        let mut buf = [0u8; (3 * 8 / 2) * 2];
        Leds::serialize_colors(&leds, buf.view_bits_mut::<Msb0>());
        assert_eq!(buf, [
            0b1100_1000, 0b1100_1000, 0b1100_1000, 0b1100_1000, // 0xaa
            0b1100_1100, 0b1100_1100, 0b1100_1100, 0b1100_1100, // 0xff
            0b1000_1000, 0b1100_1100, 0b1000_1000, 0b1000_1100, // 0x31
            0b1000_1000, 0b1100_1100, 0b1000_1000, 0b1000_1100, // 0x31
            0b1100_1000, 0b1100_1000, 0b1100_1000, 0b1100_1000, // 0xaa
            0b1100_1100, 0b1100_1100, 0b1100_1100, 0b1100_1100, // 0xff
        ]);
    }

    // #[test]
    // fn serialize_all() {
    //     let mut ws2812 = Ws2812::new();
    //     for led in ws2812.leds.iter_mut() {
    //         *led = RGB8::new(30, 20, 10);
    //     }
    //     ws2812.serialize();
    //     let s = ws2812.buffer()
    //         .view_bits::<Msb0>()
    //         .iter()
    //         .map(|b| format!("{}", *b as usize))
    //         .collect::<Vec<_>>()
    //         .join("");
    //     println!("s = {}", s);
    // }
}
