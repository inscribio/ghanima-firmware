use rgb::RGB8;

use crate::bsp::{NLEDS, ws2812b, sides::BoardSide};
use super::{LedConfig, Pattern, Repeat, Phase, Transition, Interpolation};
use super::condition::KeyboardState;

pub type Leds = ws2812b::Leds<NLEDS>;

/// Generates LED colors according to current [`LedConfig`]
pub struct PatternController<'a> {
    leds: Leds,
    config: &'a LedConfig,
    patterns: [PatternExecutor<'a>; NLEDS],
    pattern_candidates: [Option<&'a Pattern>; NLEDS],
    side: BoardSide,
}

/// Generates the color for a single LED depending on current time
#[derive(Default)]
struct PatternExecutor<'a> {
    pattern: Option<PatternIter<'a>>,
    start_time: u32,
}

/// Abstracts the logic of iterating over subsequent pattern transitions
struct PatternIter<'a> {
    pattern: &'a Pattern,
    index: usize,
    rev: bool,
    prev: Option<&'a Transition>,
}

impl<'a> PatternController<'a> {
    pub fn new(side: BoardSide, config: &'a LedConfig) -> Self {
        Self {
            leds: Leds::new(),
            config,
            side,
            patterns: Default::default(),
            pattern_candidates: Default::default(),
        }
    }

    pub fn tick(&mut self, time: u32, state: &KeyboardState) -> &Leds {
        // Reset led pattern candidates
        self.pattern_candidates.fill(None);

        // Scan the rules that we might consider, rules on end of list overwrite previous ones.
        let rule_candidates = [
            &self.config.default,
            self.config.layers[state.layer as usize],
        ];
        for rules in rule_candidates {
            for rule in rules {
                rule.keys.for_each(|row, col| {
                    if rule.condition.applies(state) {
                        // Keys iterator iterates only over non-joystick coordinates
                        let led_num = self.side.led_number((row, col))
                            .unwrap();
                        self.pattern_candidates[led_num as usize] = Some(&rule.pattern);
                    }
                });
            }
        }

        // Update pattern using the final pattern candidates, then calculate current colors.
        for led in 0..NLEDS {
            self.patterns[led].update(time, self.pattern_candidates[led]);
            let color = self.patterns[led].tick(time);
            self.leds.set_gamma_corrected(led, &color);
        }

        &self.leds
    }
}

impl<'a> PatternExecutor<'a> {
    /// Set new pattern and reset its start time
    fn reset(&mut self, time: u32, pattern: Option<&'a Pattern>) {
        self.pattern = pattern.map(|p| PatternIter::new(p));
        self.start_time = time;
    }

    /// Update pattern if it is different than the current one
    pub fn update(&mut self, time: u32, pattern: Option<&'a Pattern>) {
        // Keep previous pattern if it is same one as current one (compare pointers only)
        let keep = match (self.pattern.as_ref(), pattern) {
            (Some(this), Some(other)) => {
                // Compare patterns by pointer address to determine if they are different.
                let are_same = core::ptr::eq(this.pattern(), other);
                match (are_same, &this.pattern().repeat, &other.repeat) {
                    // Always keep previous if the new one is the same as the current one.
                    (true, _, _) => true,
                    // If both are Once then interrupt the current one and use the new one.
                    (false, Repeat::Once, Repeat::Once) => false,
                    // If only current is Once than keep it until it has finished.
                    (false, Repeat::Once, _) => !this.finished(),
                    // Otherwise use the new one
                    (false, _, _) => false,
                }
            },
            (None, None) => true,
            _ => false,
        };
        if !keep {
            self.reset(time, pattern);
        }
    }

    /// Advance transitions until the one that should be running now
    fn advance_pattern(start_time: &mut u32, curr_time: u32, pattern: &mut PatternIter<'a>) {
        while let Some(transition) = pattern.curr() {
            if curr_time < *start_time + transition.duration as u32 {
                break;
            }
            *start_time += transition.duration as u32;
            pattern.advance();
        }
    }

    /// Calculate color at current time
    fn get_color(start_time: u32, curr_time: u32, pattern: &PatternIter<'a>) -> Option<RGB8> {
        let transition = pattern.curr()?;

        // Calculate transition-local time in relation to transition duration
        debug_assert!(curr_time >= start_time && curr_time < start_time + transition.duration as u32);
        let t = if pattern.is_rev() {
            (start_time + transition.duration as u32) - curr_time
        } else {
            curr_time - start_time
        } as u16;

        let curr = transition.color;
        let color = match transition.interpolation {
            Interpolation::Piecewise => curr,
            Interpolation::Linear => {
                let prev = pattern.prev().map(|t| t.color)
                    .unwrap_or(RGB8::new(0, 0, 0));
                let (prev, curr) = if pattern.is_rev() {
                    (curr, prev)
                } else {
                    (prev, curr)
                };

                // Must hold any u16 so +1 bit for sign
                type Fix = fixed::types::I17F15;
                let ratio = Fix::from_num(t) / Fix::from_num(transition.duration);

                let interpolate = |a: u8, b: u8| {
                    let (a, b) = (Fix::from_num(a), Fix::from_num(b));
                    let c = a + (b - a) * ratio;
                    c.round().to_num::<u8>()
                };
                RGB8::new(
                    interpolate(prev.r, curr.r),
                    interpolate(prev.g, curr.g),
                    interpolate(prev.b, curr.b),
                )
            },
        };

        Some(color)
    }

    /// Generate color for the current time instant
    pub fn tick(&mut self, time: u32) -> RGB8 {
        self.pattern.as_mut()
            .and_then(|pattern| {
                // Make sure transition is up-to-date, then calculate current color
                Self::advance_pattern(&mut self.start_time, time, pattern);
                Self::get_color(self.start_time, time, pattern)
            })
            // Fall back to "no color", a.k.a. RGB black
            .unwrap_or(RGB8::new(0, 0, 0))
    }
}

impl<'a> PatternIter<'a> {
    pub fn new(pattern: &'a Pattern) -> Self {
        Self {
            pattern,
            prev: None,
            // Always start in forward order
            index: 0,
            rev: false,
        }
    }

    pub fn is_rev(&self) -> bool {
        self.rev
    }

    pub fn pattern(&self) -> &'a Pattern {
        self.pattern
    }

    pub fn prev(&self) -> Option<&'a Transition> {
        self.prev
    }

    pub fn curr(&self) -> Option<&'a Transition> {
        self.pattern.transitions.get(self.index)
    }

    pub fn finished(&self) -> bool {
        self.curr().is_none()
    }

    pub fn advance(&mut self) {
        if self.pattern.transitions.len() == 0 {
            return
        }

        self.prev = self.curr();

        // Repetition logic
        match self.pattern.repeat {
            Repeat::Once => {
                if self.index < self.pattern.transitions.len() {
                    self.index += 1;
                }
            },
            Repeat::Wrap => {
                self.index = (self.index + 1) % self.pattern.transitions.len();
            },
            Repeat::Reflect => {
                if self.rev {
                    if self.index > 0 {
                        self.index -= 1;
                    } else {
                        self.rev = false;
                        self.index = 1;
                    }
                } else {
                    self.index += 1;
                    if self.index == self.pattern.transitions.len() {
                        self.index -= 2;
                        self.rev = true;
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify tuples (prev_index, curr_index, is_rev), .advance() in between.
    fn test_pattern_iter(repeat: Repeat, expect: &[(Option<usize>, Option<usize>, bool)]) {
        static TRANSITIONS: &[Transition] = &[
            Transition { color: RGB8::new(1, 1, 1), duration: 1000, interpolation: Interpolation::Linear },
            Transition { color: RGB8::new(2, 2, 2), duration: 1000, interpolation: Interpolation::Linear },
            Transition { color: RGB8::new(3, 3, 3), duration: 1000, interpolation: Interpolation::Linear },
            Transition { color: RGB8::new(4, 4, 4), duration: 1000, interpolation: Interpolation::Linear },
        ];
        let pattern = Pattern {
            repeat,
            transitions: TRANSITIONS,
            phase: Phase { x: 0.0, y: 0.0 }
        };

        let mut iter = PatternIter::new(&pattern);
        let verify = |step: usize, iter: &PatternIter, (prev, curr, rev): &(Option<usize>, Option<usize>, bool)| {
            assert_eq!(iter.prev(), prev.map(|i| &TRANSITIONS[i]), "Step {}: prev", step);
            assert_eq!(iter.curr(), curr.map(|i| &TRANSITIONS[i]), "Step {}: curr", step);
            assert_eq!(iter.is_rev(), *rev, "Step {}: is_rev", step);
        };

        verify(0, &iter, &expect[0]);
        for (i, step) in expect[1..].iter().enumerate() {
            iter.advance();
            verify(i + 1, &iter, step);
        }
    }

    #[test]
    fn pattern_iter_once() {
        test_pattern_iter(Repeat::Once, &[
            (None, Some(0), false),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
            (Some(2), Some(3), false),
            (Some(3), None, false),
        ]);
    }

    #[test]
    fn pattern_iter_wrap() {
        test_pattern_iter(Repeat::Wrap, &[
            (None, Some(0), false),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
            (Some(2), Some(3), false),
            (Some(3), Some(0), false),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
            (Some(2), Some(3), false),
            (Some(3), Some(0), false),
        ]);
    }

    #[test]
    fn pattern_iter_reflect() {
        test_pattern_iter(Repeat::Reflect, &[
            (None, Some(0), false),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
            (Some(2), Some(3), false),
            (Some(3), Some(2), true),
            (Some(2), Some(1), true),
            (Some(1), Some(0), true),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
            (Some(2), Some(3), false),
        ]);
    }

    static PATTERNS: &[Pattern] = &[
        Pattern {
            repeat: Repeat::Once,
            phase: Phase { x: 0.0, y: 0.0 },
            transitions: &[
                Transition { color: RGB8::new(100, 100, 100), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(200, 200, 200), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(250, 250, 250), duration: 1000, interpolation: Interpolation::Linear },
            ],
        },
        Pattern {
            repeat: Repeat::Wrap,
            phase: Phase { x: 0.0, y: 0.0 },
            transitions: &[
                Transition { color: RGB8::new(40, 40, 40), duration: 1000, interpolation: Interpolation::Piecewise },
                Transition { color: RGB8::new(50, 50, 50), duration: 1000, interpolation: Interpolation::Piecewise },
                Transition { color: RGB8::new(60, 60, 60), duration: 1000, interpolation: Interpolation::Piecewise },
            ],
        },
        Pattern {
            repeat: Repeat::Reflect,
            phase: Phase { x: 0.0, y: 0.0 },
            transitions: &[
                Transition { color: RGB8::new(0, 0, 100), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(0, 0, 200), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(0, 0, 250), duration: 1000, interpolation: Interpolation::Linear },
            ],
        },
    ];

    #[test]
    fn pattern_executor_update_only_if_pattern_changed() {
        let mut exec = PatternExecutor::default();
        assert!(exec.pattern.is_none());
        assert_eq!(exec.start_time, 0);

        exec.update(1000, None);
        assert!(exec.pattern.is_none());
        assert_eq!(exec.start_time, 0);

        exec.update(2000, Some(&PATTERNS[1]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[1]));
        assert_eq!(exec.start_time, 2000);

        exec.update(3000, Some(&PATTERNS[1]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[1]));
        assert_eq!(exec.start_time, 2000);

        exec.update(4000, Some(&PATTERNS[1]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[1]));
        assert_eq!(exec.start_time, 2000);

        exec.update(5000, Some(&PATTERNS[2]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[2]));
        assert_eq!(exec.start_time, 5000);
    }

    #[test]
    fn pattern_executor_keep_until_finished() {
        let mut exec = PatternExecutor::default();
        assert!(matches!(PATTERNS[0].repeat, Repeat::Once));

        exec.update(0, Some(&PATTERNS[0]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[0]));

        exec.tick(100); exec.update(100, Some(&PATTERNS[1]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[0]));

        exec.tick(1100); exec.update(1100, Some(&PATTERNS[1]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[0]));

        exec.tick(2100); exec.update(2100, Some(&PATTERNS[1]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[0]));

        // Now the new pattern will be set as pattern 0 has finished.
        exec.tick(3100); exec.update(3100, Some(&PATTERNS[1]));
        assert!(core::ptr::eq(exec.pattern.as_ref().unwrap().pattern, &PATTERNS[1]));
    }

    fn test_pattern_executor_advance(pattern: &Pattern, seq: &[(u32, (u32, Option<usize>))]) {
        let mut iter = PatternIter::new(&pattern);
        let mut start_time = 0;

        for (t_curr, (t_start, transition)) in seq {
            PatternExecutor::advance_pattern(&mut start_time, *t_curr, &mut iter);
            let curr = iter.curr();
            match transition {
                None => assert!(curr.is_none(), "t = {}", *t_curr),
                Some(i) => assert!(core::ptr::eq(curr.unwrap(), &iter.pattern().transitions[*i]), "t = {}", *t_curr),
            }
            assert_eq!(start_time, *t_start);
        }
    }

    #[test]
    fn pattern_executor_advance_pattern_by_1() {
        test_pattern_executor_advance(&PATTERNS[0], &[
            (0, (0, Some(0))),
            (500, (0, Some(0))),
            (1000, (1000, Some(1))),
            (1800, (1000, Some(1))),
            (2100, (2000, Some(2))),
            (3100, (3000, None)),
        ]);
    }

    #[test]
    fn pattern_executor_advance_pattern_by_many() {
        test_pattern_executor_advance(&PATTERNS[0], &[
            (500, (0, Some(0))),
            (3100, (3000, None)),
        ]);
    }

    #[test]
    fn pattern_executor_advance_pattern_wrap() {
        test_pattern_executor_advance(&PATTERNS[1], &[
            (0, (0, Some(0))),
            (1000, (1000, Some(1))),
            (2100, (2000, Some(2))),
            (3100, (3000, Some(0))),
            (6100, (6000, Some(0))),
        ]);
    }

    #[test]
    fn pattern_executor_advance_pattern_reflect() {
        test_pattern_executor_advance(&PATTERNS[2], &[
            (0, (0, Some(0))),
            (1000, (1000, Some(1))),
            (2100, (2000, Some(2))),
            (3100, (3000, Some(1))),
            (4100, (4000, Some(0))),
            (5100, (5000, Some(1))),
        ]);
    }

    fn test_pattern_executor_colors(pattern: &Pattern, seq: &[(u32, Option<RGB8>)]) {
        let mut iter = PatternIter::new(pattern);
        let mut start_time = 0;
        for (time, color) in seq {
            PatternExecutor::advance_pattern(&mut start_time, *time, &mut iter);
            assert_eq!(&PatternExecutor::get_color(start_time, *time, &iter), color, "t = {}", *time);
        }
    }

    #[test]
    fn pattern_executor_get_color_piecewise() {
        // Should always show current transition's "target" color
        static PATTERN: Pattern = Pattern {
            repeat: Repeat::Reflect,
            phase: Phase { x: 0.0, y: 0.0 },
            transitions: &[
                Transition { color: RGB8::new(1, 1, 1), duration: 1000, interpolation: Interpolation::Piecewise },
                Transition { color: RGB8::new(2, 2, 2), duration: 1000, interpolation: Interpolation::Piecewise },
                Transition { color: RGB8::new(3, 3, 3), duration: 1000, interpolation: Interpolation::Piecewise },
            ],
        };
        test_pattern_executor_colors(&PATTERN, &[
            (0, Some(RGB8::new(1, 1, 1))),
            (500, Some(RGB8::new(1, 1, 1))),
            (1300, Some(RGB8::new(2, 2, 2))),
            (2300, Some(RGB8::new(3, 3, 3))),
            (3300, Some(RGB8::new(2, 2, 2))),
            (4300, Some(RGB8::new(1, 1, 1))),
            (5300, Some(RGB8::new(2, 2, 2))),
        ]);
    }

    #[test]
    fn pattern_executor_get_color_linear_wrap() {
        // Should always be the color between the current one nad the previous one
        static PATTERN: Pattern = Pattern {
            repeat: Repeat::Wrap,
            phase: Phase { x: 0.0, y: 0.0 },
            transitions: &[
                Transition { color: RGB8::new(100, 100, 100), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(200, 200, 200), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(240, 240, 240), duration: 1000, interpolation: Interpolation::Linear },
            ],
        };
        test_pattern_executor_colors(&PATTERN, &[
            (0, Some(RGB8::new(0, 0, 0))),
            (500, Some(RGB8::new(50, 50, 50))),
            (800, Some(RGB8::new(80, 80, 80))),
            (995, Some(RGB8::new(99, 99, 99))),
            (996, Some(RGB8::new(100, 100, 100))),  // due to rounding
            (1000, Some(RGB8::new(100, 100, 100))),
            (1500, Some(RGB8::new(150, 150, 150))),
            (2500, Some(RGB8::new(220, 220, 220))),
            (3000, Some(RGB8::new(240, 240, 240))),
            (3500, Some(RGB8::new(170, 170, 170))),  // half in between 240 and 100
            (4000, Some(RGB8::new(100, 100, 100))),
            (4500, Some(RGB8::new(150, 150, 150))),
        ]);
    }

    #[test]
    fn pattern_executor_get_color_linear_reflect() {
        // Should always be the color between the current one nad the previous one
        static PATTERN: Pattern = Pattern {
            repeat: Repeat::Reflect,
            phase: Phase { x: 0.0, y: 0.0 },
            transitions: &[
                Transition { color: RGB8::new(100, 100, 100), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(200, 200, 200), duration: 1000, interpolation: Interpolation::Linear },
                Transition { color: RGB8::new(240, 240, 240), duration: 1000, interpolation: Interpolation::Linear },
            ],
        };
        test_pattern_executor_colors(&PATTERN, &[
            (0, Some(RGB8::new(0, 0, 0))),
            (2500, Some(RGB8::new(220, 220, 220))),
            (3000, Some(RGB8::new(240, 240, 240))),
            (3500, Some(RGB8::new(220, 220, 220))),  // half in between 240 and 200
            (4000, Some(RGB8::new(200, 200, 200))),
            (4500, Some(RGB8::new(150, 150, 150))),
            (5000, Some(RGB8::new(100, 100, 100))),
            (5500, Some(RGB8::new(150, 150, 150))),
            (6000, Some(RGB8::new(200, 200, 200))),
        ]);
    }
}
