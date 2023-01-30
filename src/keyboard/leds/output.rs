use crate::bsp::{sides::{PerSide, BoardSide}, ws2812b, NLEDS, LedColors};

use super::LedController;

pub type Leds = ws2812b::Leds<NLEDS>;

/// Storage for LED colors with option to overwrite output for given time
pub struct LedOutput {
    leds: Leds,
    mode: OutputMode,
}

/// How we actually generate output colors
enum OutputMode {
    /// Generate colors from LED pattern controller ticks
    Controller,
    /// Overwriting colors for some duration, e.g. to signalize error
    Overwrite { ticks: u16 },
    /// Using colors received from other half over UART
    FromOther,
}

impl LedOutput {
    pub const fn new() -> Self {
        Self {
            leds: Leds::new(),
            mode: OutputMode::Controller,
        }
    }

    /// Configure pattern overwrite for given duration
    ///
    /// This returns [`Leds`] which should be manually configured
    /// by setting required colors. Normal patterns will not be used
    /// ([`Leds`] will not be modified) for the duration of `ticks`.
    pub fn set_overwrite(&mut self, ticks: u16) -> &mut Leds {
        self.mode = OutputMode::Overwrite { ticks };
        &mut self.leds
    }

    // // TODO: better name - setting values received from other keyboard half
    // pub fn use_from_other(&mut self, side: BoardSide, colors: &LedColors) {
    //     self.leds[side].colors = colors.clone();
    //     self.mode = OutputMode::FromOther;
    // }

    pub fn use_from_controller(&mut self) {
        self.mode = OutputMode::Controller;
    }

    /// Generate colors for current time returning [`Leds`] ready for serialization
    ///
    /// This must be called periodically to correctly track overwrites time.
    pub fn tick(&mut self, time: u32, controller: &mut LedController) -> &Leds {
        if let OutputMode::Overwrite { ticks } = self.mode {
            if ticks == 0 {
                self.mode = OutputMode::Controller;
            } else {
                self.mode = OutputMode::Overwrite { ticks: ticks - 1 };
            }
        }

        if let OutputMode::Controller = self.mode {
            // TODO: todo!("Track if colors changed to avoid re-sending data when not needed");
            controller.tick(time, &mut self.leds);
        }

        &self.leds
    }

    pub fn current(&self) -> &Leds {
        &self.leds
    }
}
