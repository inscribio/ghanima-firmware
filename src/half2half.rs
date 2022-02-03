use defmt::Format;
use serde::{Serialize, Deserialize};
use postcard::{flavors::{Slice, Cobs}, CobsAccumulator};
use heapless::Vec;

use crate::hal_ext::{crc, ChecksumGen};

#[derive(Serialize, Deserialize, Format)]
pub enum Message {
    EstablishMaster,
    ReleaseMaster,
    Ack,
}

impl Message {
    pub fn to_slice<'a>(&self, crc: &mut crc::Crc, buf: &'a mut [u8]) -> postcard::Result<&'a mut [u8]> {
        crc.start().serialize_to_slice_cobs::<Self>(self, buf)
    }

    pub fn to_vec<'a, const N: usize>(&self, crc: &mut crc::Crc) -> postcard::Result<Vec<u8, N>> {
        crc.start().serialize_to_vec_cobs::<Self, N>(self)
    }

    // TODO: maybe try to somehow return `impl Iterator<Item=Message>`
    pub fn from_slice<F, const N: usize>(
        cobs_acc: &mut CobsAccumulator<N>,
        data: &[u8],
        mut callback: F,
    )
    where
        F: FnMut(Message)
    {
        use postcard::FeedResult::*;
        let mut window = data;
        while !window.is_empty() {
            window = match cobs_acc.feed::<Self>(&window) {
                Consumed => break,
                OverFull(new_window) => new_window,
                DeserError(new_window) => new_window,
                Success { data, remaining } => {
                    callback(data);
                    remaining
                }
            }
        }
    }
}
