#![cfg_attr(target_os = "none", no_std)]

use static_assertions as sa;
use bitvec::{BitArr, order::Msb0, view::BitView, slice::BitSlice, bits, array::BitArray, bitarr};
use rgb::{RGB8, ComponentSlice};

const LEDS_COUNT: usize = 28;
const RGB_BITS: usize = 3 * 8;

// SPI frequency: 3 MHz; Bit time: 333 ns
const T0H_BITS: usize = 1;  // 333 ns (vs 220-380 ns)
const T0L_BITS: usize = 3;  // 1000 ns (vs 580-1000 ns)
const T1H_BITS: usize = 2;  // 666 ns (vs 580-1000 ns)
const T1L_BITS: usize = 2;  // 666 ns (vs 580-1000 ns)
const RESET_US: usize = 280;

// Currently assuming we use the same bit count for 0 and 1.
// This allows to index buffer with serialized data.
sa::const_assert_eq!(T0L_BITS + T0H_BITS, T1L_BITS + T1H_BITS);
const SERIAL_BITS: usize = T0L_BITS + T0H_BITS;

const N: usize = LEDS_COUNT * RGB_BITS * SERIAL_BITS;
const N_BITS: usize = N + RESET_US * 3 * 2;

    // Data for each LED with 3x8=24-bit RGB color, with each bit serialized as X bits.
pub type BitBuffer = BitArr!(for LEDS_COUNT * RGB_BITS * SERIAL_BITS + (RESET_US * 3 * 2), in u8, Msb0);

const BUF_SIZE: usize = bitvec::mem::elts::<u8>(N_BITS);

pub struct Ws2812 {
    pub leds: [RGB8; LEDS_COUNT],
}

impl Ws2812 {
    pub const fn new() -> Self {
        Self {
            leds: [RGB8::new(0, 0, 0); LEDS_COUNT],
        }
    }

    fn serialize_colors(colors: &[RGB8], buf: &mut BitSlice<u8, Msb0>) {
        let mut chunks = unsafe {
            buf[RESET_US*3..RESET_US*3+N].chunks_exact_mut(SERIAL_BITS)
                .remove_alias()
        };

        let one = bitarr![u8, Msb0; 1, 1, 0, 0];
        let zero = bitarr![u8, Msb0; 1, 0, 0, 0];

        for rgb in colors {
            let grb = [rgb.g, rgb.r, rgb.b];
            for bit in grb.view_bits::<Msb0>() {
                let chunk = chunks.next().unwrap();
                chunk.copy_from_bitslice(if *bit {
                    &one[..SERIAL_BITS]
                } else {
                    &zero[..SERIAL_BITS]
                });
            }
        }
    }

    // buf must be large enough (underlying buffer of BitBuffer)
    pub fn serialize(&mut self, buf: &mut [u8]) {
        Self::serialize_colors(&self.leds, buf.view_bits_mut())
    }

    // pub fn buffer(&self) -> &[u8] {
    //     self.buf.as_raw_slice()
    // }
    //
    // // TODO: remove this?
    // pub fn buffer_mut(&mut self) -> &mut [u8] {
    //     self.buf.as_raw_mut_slice()
    // }
}

// pub const fn buffer_size() -> usize {
//     let zero = T0L_BITS + T0H_BITS;
//     let one = T1L_BITS + T1H_BITS;
//     let buffer_bits = RGB_BITS * LEDS_COUNT * if zero > one { zero } else { one };
//     // make sure to have space for all bits
//     (buffer_bits + 7) / 8
// }
//
// pub type LedsBuffer = [u8; buffer_size()];
//
// pub fn fill_buffer(buf: &mut LedsBuffer) {
//     use bitvec::{view::BitView, order::{Msb0, Lsb0}, bits};
//     // 4 bits for each H+L pair
//     let mut chunks = unsafe {
//         buf.view_bits_mut::<Msb0>()
//             .chunks_exact_mut(4)
//             .remove_alias()
//     };
//     for led in 0..LEDS_COUNT {
//         let red = 10;
//         let green = 20;
//         let blue = 30;
//         let bytes: [u8; 3] = [green, red, blue];
//         let bits = bytes.view_bits::<Msb0>();
//         for bit in bits {
//             let chunk = chunks.next().unwrap();
//             if *bit {
//                 chunk.copy_from_bitslice(bits![u8, Msb0; 1, 1, 0, 0]);
//             } else {
//                 chunk.copy_from_bitslice(bits![u8, Msb0; 1, 0, 0, 0]);
//             }
//         }
//     }
// }

#[cfg(test)]
mod tests {
    use std::println;

    use super::*;

    #[test]
    fn serialize_one() {
        let leds = [RGB8::new(0xff, 0xaa, 0x31)];
        let mut buf = [0u8; 3 * 8 / 2];
        Ws2812::serialize_colors(&leds, buf.view_bits_mut::<Msb0>());
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
        Ws2812::serialize_colors(&leds, buf.view_bits_mut::<Msb0>());
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
    // fn serialize_wtf() {
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

    // fn to_bits_string(buf: &[u8]) -> String {
    //     const MSB_FIRST: bool = true;
    //     let mut s = String::with_capacity(buf.len() * 8);
    //     for byte in buf {
    //         for bit in 0..8 {
    //             let bit = if MSB_FIRST { 7 - bit } else { bit };
    //             let val = byte & (1 << bit);
    //             s += if val != 0 { "1" } else { "0" };
    //         }
    //     }
    //     s
    // }
    //
    // fn add_space(s: &str, chunk: usize) -> String {
    //     s.as_bytes()
    //         .chunks(chunk)
    //         .map(|c| core::str::from_utf8(c).unwrap())
    //         .collect::<Vec<_>>()
    //         .join(" ")
    // }
    //
    // fn no_space(s: &str) -> String {
    //     s.replace(" ", "")
    // }
    //
    // #[test]
    // fn buffer_fill() {
    //     let mut led_buf = [0; buffer_size()];
    //     fill_buffer(&mut led_buf);
    //
    //     // GRB -> 20 10 30 -> 0b10100 0b01010 0b11110 (and 1 -> 1100, 0 -> 1000)
    //     let expected = concat!(
    //         "1000 1000 1000 1100 1000 1100 1000 1000",  // green
    //         "1000 1000 1000 1000 1100 1000 1100 1000",  // red
    //         "1000 1000 1000 1100 1100 1100 1100 1000",  // blue
    //         );
    //     let got = add_space(&to_bits_string(&led_buf[..(3 * 8 * 4 / 8)]), 8);
    //
    //     assert_eq!(got, add_space(&no_space(expected), 8));
    // }
}
