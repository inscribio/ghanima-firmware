use rgb::{RGB8, ComponentMap};

use crate::bsp::sides::PerSide;
use crate::bsp::{NLEDS, sides::BoardSide};
use crate::keyboard::actions::Inc;
use crate::utils::CircularIter;
use super::output::Leds;
use super::{LedConfig, Pattern, Repeat, Transition, Interpolation, LedConfigurations, LedsBitset};
use super::condition::{KeyboardState, RuleKeys, KeyActionCache};

/// Generates LED colors according to current [`LedConfig`]
pub struct LedController<'a> {
    config: CircularIter<'a, LedConfig>,
    actions: &'a [KeyActionCache],
    patterns: PerSide<[ColorGenerator<'a>; NLEDS]>,
    pattern_candidates: PerSide<[Option<&'a Pattern>; NLEDS]>,
    brightness: u8,
    last_time: Option<u32>, // for calculating time delta from last tick
}

/// Generates the color for a single LED depending on current time
#[derive(Default)]
struct ColorGenerator<'a> {
    pattern: Option<PatternIter<'a>>,
    remaining_time: u16,  // down-counter from transition.duration
    once_should_reset: bool,
}

/// Abstracts the logic of iterating over subsequent pattern transitions
struct PatternIter<'a> {
    pattern: &'a Pattern,
    index: u8,
    rev: bool,
    prev: Option<u8>,
}

impl<'a> LedController<'a> {
    pub const INITIAL_BRIGHTNESS: u8 = (u8::MAX as u16 * 2 / 3) as u8;

    pub fn new(configurations: &'a LedConfigurations, actions: &'a [KeyActionCache]) -> Self {
        Self {
            config: CircularIter::new(configurations),
            actions,
            patterns: Default::default(),
            pattern_candidates: Default::default(),
            brightness: Self::INITIAL_BRIGHTNESS,
            last_time: None,
        }
    }

    fn next_time_delta(&mut self, time: u32) -> u16 {
        let time_delta = self.last_time
            .map(|last| time.wrapping_sub(last)) // handle integer wrapping
            .unwrap_or(0) // assume delta 0 on first run
            .try_into() // handle case when >u16 even though it is very unlikely
            .unwrap_or(u16::MAX);
        self.last_time = Some(time);
        time_delta
    }

    /// Update currently applicable patterns based on keyboard state changes
    pub fn update_patterns(&mut self, time: u32, state_change: Option<KeyboardState>) {
        // Updating currently used patterns is costly (>500 us), but we only need
        // to update them when keyboard state changed.
        if let Some(state) = state_change {
            // Reset pattern candidates
            self.pattern_candidates.for_each(|side| side.fill(None));

            // Scan the rules that we might consider, rules on end of list overwrite previous ones.
            for rule in self.config.current().iter() {
                for side in BoardSide::EACH {
                    let leds = rule.condition.applies_to(&state, &side, self.actions);
                    // Optimization: avoid iteration over keys when not needed
                    if leds.is_none() {
                        // Not applicable to any led - skip
                    } else if leds.is_all() && rule.keys.is_none() {
                        // Applicable to all leds and to all keys, so just fill whole array
                        self.pattern_candidates[side].fill(Some(&rule.pattern));
                    } else {
                        // More complicated situation - scan all leds
                        rule.keys.for_each_led(|led_num| {
                            if leds.is_pressed(led_num) {
                                self.pattern_candidates[side][led_num as usize] = Some(&rule.pattern);
                            }
                        });
                    }
                }
            }
        }

        let time_delta = self.next_time_delta(time);
        for side in BoardSide::EACH {
            for led in 0..NLEDS {
                self.patterns[side][led].update(time_delta, self.pattern_candidates[side][led]);
            }
        }
    }

    /// Generate colors for current time, returning [`Leds`] ready for serialization
    pub fn tick(&mut self, time: u32, leds: &mut PerSide<Leds>) -> PerSide<LedsBitset> {
        let time_delta = self.next_time_delta(time);
        let mut modified: PerSide<LedsBitset> = Default::default();

        for side in BoardSide::EACH {
            debug_assert_eq!(self.patterns[side].len(), leds[side].colors.len());
            let patterns = self.patterns[side].iter_mut();
            let leds = leds[side].colors.iter_mut();

            for (i, (pattern, led)) in patterns.zip(leds).enumerate() {
                let new = pattern.tick(time_delta)
                    .map(|channel| Self::dimmed(channel, self.brightness))
                    .map(Leds::gamma_correction);
                if new != *led {
                    modified[side].set(i as u8, true);
                }
                *led = new;
            }
        }

        modified
    }

    fn dimmed(color: u8, brightness: u8) -> u8 {
        (((brightness as u16 + 1) * color as u16) >> 8) as u8
    }

    /// Change current configuration
    ///
    /// Note that [`Self::update_patterns`] must be called to actually
    /// reset patterns to use the new configuration.
    pub fn cycle_config(&mut self, inc: Inc) {
        inc.update(&mut self.config);
    }

    /// Get current global brightness
    pub fn brightness(&self) -> u8 {
        self.brightness
    }

    /// Change global brightness
    pub fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness;
    }
}

impl<'a> ColorGenerator<'a> {
    /// Set new pattern and reset its start time
    fn reset(&mut self, pattern: Option<&'a Pattern>) {
        self.pattern = pattern.map(PatternIter::new);
        self.once_should_reset = false;
        self.remaining_time = Self::initial_remaining_time(self.pattern.as_ref());
    }

    fn initial_remaining_time(pattern_iter: Option<&PatternIter<'a>>) -> u16 {
        pattern_iter
            .and_then(|piter| piter.curr())
            .map(|t| t.duration)
            .unwrap_or(0)
    }

    /// Update pattern if it is different than the current one
    pub fn update(&mut self, time_delta: u16, pattern: Option<&'a Pattern>) {
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
            self.reset(pattern);
        } else if let Some(pattern) = self.pattern.as_mut() {
            Self::advance_pattern(&mut self.remaining_time, time_delta, pattern);
        }
    }

    /// Advance transitions until the one that should be running now
    fn advance_pattern(remaining_time: &mut u16, mut time_delta: u16, pattern: &mut PatternIter<'a>) {
        while let Some(transition) = pattern.curr() {
            // Duration 0 means that this is endless transition
            if transition.duration == 0 {
                return;
            }
            if time_delta < *remaining_time {
                // don't change to next transition, just decrease remaining time for this one
                *remaining_time -= time_delta;
                break;
            } else {
                // next transition, subtract remaining time for this transition from time delta
                time_delta -= *remaining_time;
                pattern.advance();
                *remaining_time = Self::initial_remaining_time(Some(pattern));
            }
        }
    }

    /// Interpolate between two colors: c1 happens at t1, c2 at t1+duration
    fn interpolate(time_delta: u16, duration: u16, c1: RGB8, c2: RGB8) -> RGB8 {
        // Must hold any u8 +1 bit for sign
        type Fix16 = fixed::types::U8F8;
        type Fix32 = fixed::types::U24F8;

        // Calculate transition-local time in relation to transition duration
        let ratio = Fix32::from_num(time_delta) / Fix32::from_num(duration);
        let ratio = Fix16::from_num(ratio);

        let channel = |a: u8, b: u8| {
            let (a, b, ratio) = if a < b {
                (a, b, ratio)
            } else {
                (b, a, Fix16::from_num(1) - ratio)
            };
            let (a, b) = (Fix16::from_num(a), Fix16::from_num(b));
            let c = a + (b - a) * ratio;
            c.round().to_num::<u8>()
        };

        RGB8::new(
            channel(c1.r, c2.r),
            channel(c1.g, c2.g),
            channel(c1.b, c2.b),
        )
    }

    // This is a bit faster than using fixed fixed crate with I9F7 (640 vs 780) but much less readable.
    // When changed from I9F7 to U8F8 (with if a < b) time increased from 780 us to 875 us (but interpolation error is smaller).
    // fn interpolate(time_delta: u16, duration: u16, c1: RGB8, c2: RGB8) -> RGB8 {
    //     // Using Q9.7 signed fixed point numbers (I9F7)
    //     let ratio = (((time_delta as u32) << 7) / (duration as u32)) as u16;
    //
    //     let channel = |a: u8, b: u8| {
    //         // To U8F8
    //         let (a, b) = ((a as i16) << 7, (b as i16) << 7);
    //         let diff = b - a;
    //         let mul = (diff as i32) * (ratio as i32); // multiplication step 1
    //         let mul = mul + (1 << 6);  // rounding
    //         let mul = (mul >> 7) as i16; // back to correct base
    //         let c = a + mul;
    //         // To U8
    //         ((c + (1 << 6)) >> 7) as u8
    //     };
    //
    //     RGB8::new(
    //         channel(c1.r, c2.r),
    //         channel(c1.g, c2.g),
    //         channel(c1.b, c2.b),
    //     )
    // }

    /// Calculate color at current time
    fn get_color(remaining_time: u16, pattern: &PatternIter<'a>) -> Option<RGB8> {
        let transition = pattern.curr()?;

        // Non-transition, just use static color.
        if transition.duration == 0 {
            return Some(transition.color);
        }

        debug_assert!(remaining_time <= transition.duration);
        let curr = transition.color;

        let color = match transition.interpolation {
            Interpolation::Piecewise => curr,
            Interpolation::Linear => {
                let prev = pattern.prev().map(|t| t.color)
                    .unwrap_or_else(|| RGB8::new(0, 0, 0));
                let (prev, curr, time) = if pattern.is_rev() {
                    (curr, prev, remaining_time)
                } else {
                    (prev, curr, transition.duration - remaining_time)
                };
                Self::interpolate(time, transition.duration, prev, curr)
            },
        };

        Some(color)
    }

    /// Generate color for the current time by advancing pattern time by given time delta
    pub fn tick(&mut self, time_delta: u16) -> RGB8 {
        self.pattern.as_mut()
            .and_then(|pattern| {
                // Make sure transition is up-to-date, then calculate current color
                Self::advance_pattern(&mut self.remaining_time, time_delta, pattern);
                Self::get_color(self.remaining_time, pattern)
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

    // At most 255 transitions to save memory space, unlikely that anyone needs more...
    fn transitions_count(&self) -> u8 {
        self.pattern.transitions.len()
            .min((u8::MAX - 1) as usize) as u8
    }

    pub fn is_rev(&self) -> bool {
        self.rev
    }

    pub fn pattern(&self) -> &'a Pattern {
        self.pattern
    }

    pub fn prev(&self) -> Option<&'a Transition> {
        self.prev.and_then(|i| self.pattern.transitions.get(i as usize))
    }

    pub fn curr(&self) -> Option<&'a Transition> {
        self.pattern.transitions.get(self.index as usize)
    }

    pub fn finished(&self) -> bool {
        self.curr().is_none()
    }

    pub fn advance(&mut self) {
        if self.pattern.transitions.is_empty() {
            return
        }

        self.prev = Some(self.index);

        // Repetition logic
        match self.pattern.repeat {
            Repeat::Once => {
                if self.index < self.transitions_count() {
                    self.index = self.index.saturating_add(1);
                }
            },
            Repeat::Wrap => {
                self.index = self.index.saturating_add(1) % self.transitions_count();
            },
            Repeat::Reflect => {
                if self.rev {
                    if self.index > 0 {
                        self.index -= 1;
                    } else {
                        self.rev = false;
                        self.index = 1 % self.transitions_count();
                    }
                } else {
                    self.index = self.index.saturating_add(1);
                    if self.index >= self.transitions_count() {
                        self.index = self.index.saturating_sub(2);
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
    use std::vec::Vec;

    use super::*;

    // Verify tuples (prev_index, curr_index, is_rev), .advance() in between.
    fn test_pattern_iter(transitions_count: usize, repeat: Repeat, expect: &[(Option<usize>, Option<usize>, bool)]) {
        static TRANSITIONS: &[Transition] = &[
            Transition { color: RGB8::new(1, 1, 1), duration: 1000, interpolation: Interpolation::Linear },
            Transition { color: RGB8::new(2, 2, 2), duration: 1000, interpolation: Interpolation::Linear },
            Transition { color: RGB8::new(3, 3, 3), duration: 1000, interpolation: Interpolation::Linear },
            Transition { color: RGB8::new(4, 4, 4), duration: 1000, interpolation: Interpolation::Linear },
        ];
        assert!(transitions_count <= TRANSITIONS.len());
        let transitions = &TRANSITIONS[..transitions_count];

        let pattern = Pattern {
            repeat,
            transitions: transitions,
            phase: Phase { x: 0.0, y: 0.0 }
        };

        let mut iter = PatternIter::new(&pattern);
        let verify = |step: usize, iter: &PatternIter, (prev, curr, rev): &(Option<usize>, Option<usize>, bool)| {
            assert_eq!(iter.prev(), prev.map(|i| &transitions[i]), "Step {}: prev", step);
            assert_eq!(iter.curr(), curr.map(|i| &transitions[i]), "Step {}: curr", step);
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
        test_pattern_iter(4, Repeat::Once, &[
            (None, Some(0), false),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
            (Some(2), Some(3), false),
            (Some(3), None, false),
        ]);
    }

    #[test]
    fn pattern_iter_wrap() {
        test_pattern_iter(4, Repeat::Wrap, &[
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
        test_pattern_iter(4, Repeat::Reflect, &[
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

    #[test]
    fn pattern_iter_reflect_small_count() {
        test_pattern_iter(3, Repeat::Reflect, &[
            (None, Some(0), false),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
            (Some(2), Some(1), true),
            (Some(1), Some(0), true),
            (Some(0), Some(1), false),
            (Some(1), Some(2), false),
        ]);
        test_pattern_iter(2, Repeat::Reflect, &[
            (None, Some(0), false),
            (Some(0), Some(1), false),
            (Some(1), Some(0), true),
            (Some(0), Some(1), false),
            (Some(1), Some(0), true),
        ]);
        test_pattern_iter(1, Repeat::Reflect, &[
            (None, Some(0), false),
            (Some(0), Some(0), true),
            (Some(0), Some(0), false),
            (Some(0), Some(0), true),
            (Some(0), Some(0), false),
        ]);
        test_pattern_iter(0, Repeat::Reflect, &[
            (None, None, false),
            (None, None, false),
            (None, None, false),
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
        Tick(u32), // absolute time
        Update(u32, Option<usize>), // absolute time, pattern num
        Expect(u16, Option<usize>), // remaining time, pattern num
    }

    fn next_time_delta(t: u32, last_time: &mut u32) -> u16 {
        assert!(t >= *last_time, "Tick is absolute time");
        let dt = (t - *last_time).try_into().unwrap();
        println!("last={last_time:} t={t:} dt={dt:}");
        *last_time = t;
        dt
    }

    fn test_pattern_update(seq: &[UpdateStep]) {
        let mut exec = ColorGenerator::default();
        assert!(exec.pattern.is_none());
        assert_eq!(exec.remaining_time, 0);

        let mut last_time = 0;

        for (i, step) in seq.iter().enumerate() {
            match step {
                UpdateStep::Tick(t) => { exec.tick(next_time_delta(*t, &mut last_time)); },
                UpdateStep::Update(t, pattern) => exec.update(next_time_delta(*t, &mut last_time), pattern.map(|pi| &PATTERNS[pi])),
                UpdateStep::Expect(remaining, pattern) => {
                    match pattern {
                        None => assert!(exec.pattern.is_none(), "step {}", i),
                        Some(pi) => {
                            let pattern = exec.pattern.as_ref().unwrap().pattern;
                            let found = PATTERNS.iter().position(|p| core::ptr::eq(pattern, p));
                            assert_eq!(found, Some(*pi), "step {}", i)
                        },
                    }
                    assert_eq!(exec.remaining_time, *remaining, "step {}", i);
                },
            }
        }
    }

    #[test]
    fn pattern_executor_update_remaining_time_on_new_pattern() {
        // Start time should change only after a new pattern has been set.
        use UpdateStep::*;
        test_pattern_update(&[
            Update(10, None),    Expect(   0, None),    Tick(11), Expect(  0, None),
            Update(20, Some(1)), Expect(1000, Some(1)), Tick(21), Expect(999, Some(1)),
            Update(30, Some(1)), Expect( 990, Some(1)), Tick(31), Expect(989, Some(1)),
            Update(40, Some(1)), Expect( 980, Some(1)), Tick(41), Expect(979, Some(1)),
            Update(50, Some(2)), Expect(1000, Some(2)), Tick(51), Expect(999, Some(2)),
        ]);
    }

    #[test]
    fn pattern_executor_keep_until_finished() {
        // New pattern should not be set if the current one is Repeat::Once.
        assert!(matches!(PATTERNS[0].repeat, Repeat::Once));
        use UpdateStep::*;
        test_pattern_update(&[
            Update(   0, Some(0)), Tick(   1), Expect( 999, Some(0)), // trans 0
            Update( 100, Some(1)), Tick( 200), Expect( 800, Some(0)), // trans 0
            Update(1100, Some(1)), Tick(1200), Expect( 800, Some(0)), // trans 1
            Update(2100, Some(1)), Tick(2200), Expect( 800, Some(0)), // trans 2
            Update(3100, Some(1)), Tick(3101), Expect(   0, Some(0)),
            // Now the new pattern will be set as pattern 0 has finished.
            Update(3200, Some(1)), Expect(1000, Some(1)),
        ]);
    }

    #[test]
    fn pattern_executor_restart_interrupted() {
        // Repeat::Once pattern should be restarted if there was a change during its execution.
        assert!(matches!(PATTERNS[0].repeat, Repeat::Once));
        use UpdateStep::*;
        test_pattern_update(&[
            Update(   0, Some(0)), Tick(   1), Expect( 999, Some(0)),
            Update( 100, Some(0)), Tick( 101), Expect( 899, Some(0)),
            // New pattern for a moment but pattern 0 is kept.
            Update(1100, Some(1)), Tick(1101), Expect( 899, Some(0)),
            // Now back to pattern 0 - it will be restarted to start_time from Update.
            Update(2100, Some(0)), Tick(2101), Expect( 999, Some(0)),
            Update(3100, Some(0)), Tick(3101), Expect( 999, Some(0)),
        ]);
    }

    fn test_pattern_executor_advance(pattern: &Pattern, seq: &[(u32, (u16, Option<usize>))]) {
        let mut iter = PatternIter::new(&pattern);
        let mut remaining_time = iter.curr().unwrap().duration;
        let mut last_time = 0;

        for (t_curr, (t_remaining, transition)) in seq {
            let dt = next_time_delta(*t_curr, &mut last_time);
            ColorGenerator::advance_pattern(&mut remaining_time, dt, &mut iter);
            let curr = iter.curr();
            match transition {
                None => assert!(curr.is_none(), "t = {}", *t_curr),
                Some(i) => assert!(
                    core::ptr::eq(curr.unwrap(), &iter.pattern().transitions[*i]),
                    "t = {}, current transition = {}", *t_curr,
                    unsafe {
                        (curr.unwrap() as *const Transition).offset_from(&iter.pattern().transitions[0] as *const Transition)
                    }
                ),
            }
            assert_eq!(remaining_time, *t_remaining);
        }
    }

    #[test]
    fn pattern_executor_advance_pattern_by_1() {
        test_pattern_executor_advance(&PATTERNS[0], &[
            (   0, (1000, Some(0))),
            ( 500, ( 500, Some(0))),
            (1000, (1000, Some(1))),
            (1800, ( 200, Some(1))),
            (2100, ( 900, Some(2))),
            (3100, (   0, None)),
        ]);
    }

    #[test]
    fn pattern_executor_advance_pattern_by_many() {
        test_pattern_executor_advance(&PATTERNS[0], &[
            ( 500, (500, Some(0))),
            (2100, (900, Some(2))),
        ]);
    }

    #[test]
    fn pattern_executor_advance_pattern_by_all() {
        test_pattern_executor_advance(&PATTERNS[0], &[
            ( 500, (500, Some(0))),
            (3100, (  0, None)),
        ]);
    }

    #[test]
    fn pattern_executor_advance_pattern_wrap() {
        test_pattern_executor_advance(&PATTERNS[1], &[
            (   0, (1000, Some(0))),
            (1000, (1000, Some(1))),
            (2100, ( 900, Some(2))),
            (3100, ( 900, Some(0))),
            (4100, ( 900, Some(1))),
            (5100, ( 900, Some(2))),
            (6100, ( 900, Some(0))),
        ]);
    }

    #[test]
    fn pattern_executor_advance_pattern_reflect() {
        test_pattern_executor_advance(&PATTERNS[2], &[
            (   0, (1000, Some(0))),
            (1000, (1000, Some(1))),
            (2100, ( 900, Some(2))),
            (3100, ( 900, Some(1))),
            (4100, ( 900, Some(0))),
            (5100, ( 900, Some(1))),
            (6100, ( 900, Some(2))),
            (7100, ( 900, Some(1))),
        ]);
    }

    fn test_pattern_executor_colors(pattern: &Pattern, seq: &[(u32, Option<RGB8>)]) {
        let mut iter = PatternIter::new(pattern);
        let mut remaining_time = iter.curr().unwrap().duration;
        let mut last_time = 0;
        for (time, color) in seq {
            let dt = next_time_delta(*time, &mut last_time);
            ColorGenerator::advance_pattern(&mut remaining_time, dt, &mut iter);
            assert_eq!(&ColorGenerator::get_color(remaining_time, &iter), color, "t = {}", *time);
        }
    }

    #[test]
    fn pattern_executor_get_color_piecewise() {
        // Should always show current transition's "target" color
        static PATTERN: Pattern = Pattern {
            repeat: Repeat::Reflect,
            phase: Phase { x: 0.0, y: 0.0 },
            transitions: &[
                Transition { color: RGB8::new(10, 10, 10), duration: 1000, interpolation: Interpolation::Piecewise },
                Transition { color: RGB8::new(20, 20, 20), duration: 1000, interpolation: Interpolation::Piecewise },
                Transition { color: RGB8::new(30, 30, 30), duration: 1000, interpolation: Interpolation::Piecewise },
            ],
        };
        test_pattern_executor_colors(&PATTERN, &[
            (   0, Some(RGB8::new(10, 10, 10))),
            ( 500, Some(RGB8::new(10, 10, 10))),
            (1300, Some(RGB8::new(20, 20, 20))),
            (2300, Some(RGB8::new(30, 30, 30))),
            (3300, Some(RGB8::new(20, 20, 20))),
            (4300, Some(RGB8::new(10, 10, 10))),
            (5300, Some(RGB8::new(20, 20, 20))),
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
            (996, Some(RGB8::new(99, 99, 99))),
            (997, Some(RGB8::new(100, 100, 100))),  // due to rounding
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

    #[test]
    fn color_dimming() {
        assert_eq!(LedController::dimmed(255/2, 255/4), 255/8);
        // Make sure that brightness 255 won't give color 254
        for brightness in 0..255 {
            assert_eq!(LedController::dimmed(255, brightness), brightness);
        }
        // ...and that full brightness doesn't change the color
        for color in 0..255 {
            assert_eq!(LedController::dimmed(color, 255), color);
        }
    }

    #[allow(dead_code)]
    #[derive(Debug, Default)]
    struct ErrorStats {
        min: f32,
        max: f32,
        avg: f32,
        std: f32,
        mse: f32,  // Mean squared error
    }

    fn get_interpolation_errors(duration: u16, c1: u8, c2: u8, plot: bool) -> ErrorStats {
        let mut times = Vec::new();
        let mut errors = Vec::new();
        let mut values_ref = Vec::new();
        let mut values_calc = Vec::new();

        let (rgb1, rgb2) = (RGB8::new(c1, c1, c1), RGB8::new(c2, c2, c2));
        for time in 0..=duration {
            times.push(time);
            let c_ref = {
                let (c1, c2, time, duration) = (c1 as f32, c2 as f32, time as f32, duration as f32);
                c1 + (time) / (duration) * (c2 - c1)
            };
            let rgb = ColorGenerator::interpolate(time, duration, rgb1, rgb2);
            let c_calc = rgb.r as f32;
            values_ref.push(c_ref);
            values_calc.push(c_calc);
            errors.push(c_calc - c_ref);
        }

        if plot {
            use gnuplot::{Figure, AxesCommon, Caption};
            let mut fig = Figure::new();
            fig.axes2d()
                .set_title("Interpolation error", &[])
                .set_x_label("Time", &[])
                .set_y_label("Color value", &[])
                .set_x_grid(true)
                .set_y_grid(true)
                .lines(
                    times.clone(),
                    values_ref.clone(),
                    &[Caption("reference")]
                )
                .lines(
                    times.clone(),
                    values_calc.clone(),
                    &[Caption("calculated")]
                );
            fig.show().unwrap();
        }

        let n = errors.len() as f32;
        let mean = errors.iter().sum::<f32>() / n;
        ErrorStats {
            min: errors.iter().copied().reduce(|acc, err| acc.min(err)).unwrap(),
            max: errors.iter().copied().reduce(|acc, err| acc.max(err)).unwrap(),
            avg: mean,
            std: (errors.iter().map(|err_i| (err_i - mean).powi(2)).sum::<f32>() / n).sqrt(),
            mse: values_calc.iter().zip(values_ref.iter())
                .map(|(c, r)| (c - r).powi(2)).sum::<f32>() / n,
        }
    }

    #[test]
    fn interpolation_error() {
        for duration in [10, 200, 600, 1000, 3000] {
        // for duration in [1000] {
            let e1 = get_interpolation_errors(duration, 0, 255, false);
            let e2 = get_interpolation_errors(duration, 255, 0, false);
            println!("dur {duration}:\n  {:?}\n  {:?}", e1, e2);
            assert!(e1.mse < 1.5);
            assert!(e2.mse < 1.5);
        }
    }
}
