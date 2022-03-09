use rgb::RGB8;

use crate::bsp::{NLEDS, ws2812b, sides::BoardSide};
use super::{LedConfig, Pattern, Repeat, Transition, Interpolation};
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
    once_should_reset: bool,
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

    /// Update currently applicable patterns based on keyboard state
    pub fn update_patterns(&mut self, time: u32, state: KeyboardState) {
        // Reset led pattern candidates
        self.pattern_candidates.fill(None);

        // Scan the rules that we might consider, rules on end of list overwrite previous ones.
        let rule_candidates = [
            self.config.default,
            self.config.layers[state.layer as usize],
        ];
        for rules in rule_candidates {
            for rule in rules {
                rule.keys.for_each(|row, col| {
                    // Keys iterator iterates only over non-joystick coordinates
                    let led_num = BoardSide::led_number((row, col))
                        .unwrap();
                    if rule.condition.applies(&state, &self.side, led_num) {
                        self.pattern_candidates[led_num as usize] = Some(&rule.pattern);
                    }
                });
            }
        }

        for led in 0..NLEDS {
            self.patterns[led].update(time, self.pattern_candidates[led]);
        }
    }

    /// Generate colors for current time, returning [`Leds`] ready for serialization
    pub fn tick(&mut self, time: u32) -> &Leds {
        for led in 0..NLEDS {
            let color = self.patterns[led].tick(time);
            self.leds.set_gamma_corrected(led, &color);
        }

        &self.leds
    }

    /// Change current configuration
    ///
    /// Note that [`PatternController::update_patterns`] must be called to actually
    /// reset patterns to use the new configuration.
    pub fn set_config(&mut self, config: &'a LedConfig) {
        self.config = config;
    }
}

impl<'a> PatternExecutor<'a> {
    /// Set new pattern and reset its start time
    fn reset(&mut self, time: u32, pattern: Option<&'a Pattern>) {
        self.pattern = pattern.map(PatternIter::new);
        self.start_time = time;
        self.once_should_reset = false;
    }

    /// Update pattern if it is different than the current one
    pub fn update(&mut self, time: u32, pattern: Option<&'a Pattern>) {
        let keep = match (self.pattern.as_ref(), pattern) {
            (Some(this), Some(other)) => {
                // Compare patterns by pointer address to determine if they are different.
                let are_same = core::ptr::eq(this.pattern(), other);
                match (are_same, &this.pattern().repeat, &other.repeat) {
                    // Only restart a Once pattern if there was another pattern that we ignored.
                    (true, Repeat::Once, Repeat::Once) => !self.once_should_reset,
                    // Always keep previous if the new one is the same as the current one.
                    // FIXME: cannot restart Once pattern on multiple short key presses
                    (true, _, _) => true,
                    // If both are Once then interrupt the current one and use the new one.
                    (false, Repeat::Once, Repeat::Once) => false,
                    // If only current is Once than keep it until it has finished.
                    (false, Repeat::Once, _) => {
                        self.once_should_reset = true;
                        !this.finished()
                    },
                    // Otherwise use the new one
                    (false, _, _) => false,
                }
            },
            (Some(this), None) => match this.pattern().repeat {
                // Keep current pattern until finished
                Repeat::Once => {
                    self.once_should_reset = true;
                    !this.finished()
                },
                _ => false,
            }
            (None, None) => true,
            (None, Some(_)) => false,
        };
        if !keep {
            self.reset(time, pattern);
        }
    }

    /// Advance transitions until the one that should be running now
    fn advance_pattern(start_time: &mut u32, curr_time: u32, pattern: &mut PatternIter<'a>) {
        while let Some(transition) = pattern.curr() {
            // Duration 0 means that this is endless transition
            if transition.duration == 0 {
                return;
            }
            if curr_time < *start_time + transition.duration as u32 {
                break;
            }
            *start_time += transition.duration as u32;
            pattern.advance();
        }
    }

    /// Interpolate between two colors: c1 happens at t1, c2 at t1+duration
    fn interpolate(time_delta: u16, duration: u16, c1: RGB8, c2: RGB8) -> RGB8 {
        // Must hold any u16 so +1 bit for sign
        type Fix = fixed::types::I17F15;

        // Calculate transition-local time in relation to transition duration
        let ratio = Fix::from_num(time_delta) / Fix::from_num(duration);

        let channel = |a: u8, b: u8| {
            let (a, b) = (Fix::from_num(a), Fix::from_num(b));
            let c = a + (b - a) * ratio;
            c.round().to_num::<u8>()
        };

        RGB8::new(
            channel(c1.r, c2.r),
            channel(c1.g, c2.g),
            channel(c1.b, c2.b),
        )
    }

    /// Calculate color at current time
    fn get_color(start_time: u32, curr_time: u32, pattern: &PatternIter<'a>) -> Option<RGB8> {
        let transition = pattern.curr()?;

        // Non-transition, just use static color.
        if transition.duration == 0 {
            return Some(transition.color);
        }

        debug_assert!(curr_time >= start_time && curr_time < start_time + transition.duration as u32);
        let curr = transition.color;

        let color = match transition.interpolation {
            Interpolation::Piecewise => curr,
            Interpolation::Linear => {
                let prev = pattern.prev().map(|t| t.color)
                    .unwrap_or_else(|| RGB8::new(0, 0, 0));
                let (prev, curr, time) = if pattern.is_rev() {
                    (curr, prev, (start_time + transition.duration as u32) - curr_time)
                } else {
                    (prev, curr, curr_time - start_time)
                };
                Self::interpolate(time as u16, transition.duration, prev, curr)
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
            .unwrap_or_else(|| RGB8::new(0, 0, 0))
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
        if self.pattern.transitions.is_empty() {
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
    use crate::keyboard::leds::Phase;

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

    enum UpdateStep {
        Tick(u32),
        Update(u32, Option<usize>),
        Expect(u32, Option<usize>),
    }

    fn test_pattern_update(seq: &[UpdateStep]) {
        let mut exec = PatternExecutor::default();
        assert!(exec.pattern.is_none());
        assert_eq!(exec.start_time, 0);

        for (i, step) in seq.iter().enumerate() {
            match step {
                UpdateStep::Tick(t) => { exec.tick(*t); },
                UpdateStep::Update(t, pattern) => exec.update(*t, pattern.map(|pi| &PATTERNS[pi])),
                UpdateStep::Expect(t, pattern) => {
                    match pattern {
                        None => assert!(exec.pattern.is_none(), "step {}", i),
                        Some(pi) => {
                            let pattern = exec.pattern.as_ref().unwrap().pattern;
                            let found = PATTERNS.iter().position(|p| core::ptr::eq(pattern, p));
                            assert_eq!(found, Some(*pi), "step {}", i)
                        },
                    }
                    assert_eq!(exec.start_time, *t, "step {}", i);
                },
            }
        }
    }

    #[test]
    fn pattern_executor_update_start_time_on_new_pattern() {
        // Start time should change only after a new pattern has been set.
        use UpdateStep::*;
        test_pattern_update(&[
            Update(10, None),    Tick(11), Expect( 0, None),
            Update(20, Some(1)), Tick(21), Expect(20, Some(1)),
            Update(30, Some(1)), Tick(31), Expect(20, Some(1)),
            Update(40, Some(1)), Tick(41), Expect(20, Some(1)),
            Update(50, Some(2)), Tick(51), Expect(50, Some(2)),
        ]);
    }

    #[test]
    fn pattern_executor_keep_until_finished() {
        // New pattern should not be set if the current one is Repeat::Once.
        assert!(matches!(PATTERNS[0].repeat, Repeat::Once));
        use UpdateStep::*;
        test_pattern_update(&[
            Update(   0, Some(0)), Tick(   1), Expect(   0, Some(0)),
            Update( 100, Some(1)), Tick( 101), Expect(   0, Some(0)),
            Update(1100, Some(1)), Tick(1101), Expect(1000, Some(0)),
            Update(2100, Some(1)), Tick(2101), Expect(2000, Some(0)),
            Update(3100, Some(1)), Tick(3101), Expect(3000, Some(0)),
            // Now the new pattern will be set as pattern 0 has finished.
            Update(3200, Some(1)), Expect(3200, Some(1)),
        ]);
    }

    #[test]
    fn pattern_update_restart_interrupted() {
        // Repeat::Once pattern should be restarted if there was a change during its execution.
        assert!(matches!(PATTERNS[0].repeat, Repeat::Once));
        use UpdateStep::*;
        test_pattern_update(&[
            Update(   0, Some(0)), Tick(   1), Expect(   0, Some(0)),
            Update( 100, Some(0)), Tick( 101), Expect(   0, Some(0)),
            // New pattern for a moment but pattern 0 is kept.
            Update(1100, Some(1)), Tick(1101), Expect(1000, Some(0)),
            // Now back to pattern 0 - it will be restarted to start_time from Update.
            Update(2100, Some(0)), Tick(2101), Expect(2100, Some(0)),
            Update(3100, Some(0)), Tick(3101), Expect(3100, Some(0)),
        ]);
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
