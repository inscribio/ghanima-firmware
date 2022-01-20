#!/usr/bin/env python

import re
import argparse
import dataclasses
import itertools

from math import ceil, floor


@dataclasses.dataclass()
class TimeRanges:
    """Time range constraints in seconds; high/low pulse time for transmitting 0/1"""
    t0h: (float, float)
    t0l: (float, float)
    t1h: (float, float)
    t1l: (float, float)

    def to_bits(self, baudrate: float):
        bit_time = 1 / baudrate

        def to_bit(t):
            b = (ceil(t[0] / bit_time), floor(t[1] / bit_time))
            assert b[0] <= b[1], b
            return b

        return BitRanges(
            n0h=to_bit(self.t0h),
            n0l=to_bit(self.t0l),
            n1h=to_bit(self.t1h),
            n1l=to_bit(self.t1l),
        )

    def with_margin(self, ratio):
        assert 0 < ratio < 1, ratio
        new = lambda range: ((1 - ratio) * range[0], (1 + ratio) * range[1])
        return dataclasses.replace(
            self,
            t0h=new(self.t0h),
            t0l=new(self.t0l),
            t1h=new(self.t1h),
            t1l=new(self.t1l),
        )


@dataclasses.dataclass()
class BitSpec:
    """Concrete bit counts"""
    n0h: int
    n0l: int
    n1h: int
    n1l: int

    def n0(self):
        """Number of bits used to send a 0"""
        return self.n0h + self.n0l

    def n1(self):
        """Number of bits used to send a 1"""
        return self.n1h + self.n1l


@dataclasses.dataclass()
class BitRanges:
    """Number of bits corresponding to TimeRanges"""
    n0h: (int, int)
    n0l: (int, int)
    n1h: (int, int)
    n1l: (int, int)

    def score(self, bits: BitSpec) -> float:
        # Reqire that we have same bit count for 0 and 1
        if bits.n0() != bits.n1():
            return - 1

        # Lower bit coutns are better
        def count_score(pair):
            val, range = pair
            return range[1] - val + 1

        # Values not on range edges are better
        def mid_score(pair):
            val, range = pair
            return 0 if val == range[0] or val == range[1] else 1

        pairs = [
            (bits.n0h, self.n0h),
            (bits.n0l, self.n0l),
            (bits.n1h, self.n1h),
            (bits.n1l, self.n1l),
        ]

        score = sum(map(count_score, pairs)) + sum(map(mid_score, pairs))

        # Better to have 8-divisible values
        if bits.n0() == 4:
            score += 1
        elif bits.n0() % 8 == 0:
            score += 2

        return score

    def find_scores(self) -> list[tuple[float, BitSpec]]:
        """Score all possible combinations"""
        r = lambda rng: range(rng[0], rng[1] + 1)
        prod = itertools.product(r(self.n0h), r(self.n0l), r(self.n1h), r(self.n1l))
        prod = map(lambda x: BitSpec(*x), prod)
        scored = [(self.score(bits), bits) for bits in prod]
        return sorted(scored, reverse=True, key=lambda x: x[0])

    def find_best(self) -> tuple[float, BitSpec]:
        """Use brute force to select best combination"""
        return max(self.find_scores(), key=lambda x: x[0])


TIME_UNITS = dict(s=1, ms=1e-3, us=1e-6, ns=1e-9, ps=1e-12)
FREQ_UNITS = dict(Hz=1, kHz=1e3, MHz=1e6, GHz=1e9, bps=1, kbps=1e3, Mbps=1e6)
PERCENT_UNITS = {'': 1, '%': 0.01}


def convert_unit(string, units, case_sensitive=True):
    """Convert a floating point number with unit (no space) to a number based on unit specs"""
    float_pattern = r'[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?'
    pattern = re.compile(r'^(?P<float>{})(?P<unit>.*)$'.format(float_pattern))
    match = pattern.match(string.strip())
    assert match, f'Wrong time string: {string}'
    num = float(match.group('float'))
    unit = match.group('unit')
    if not case_sensitive:
        unit = unit.lower()
        units = {u.lower(): v for u, v in units.items()}
    assert unit in units, f'Unexpected unit "{unit}", use one of: {", ".join(list(units.keys()))}'
    return num * units[unit]


def main(args=None):
    parser = argparse.ArgumentParser(description='''
        Calculate SPI bit counts for WS2812 transmission.
        Specify parameter values with correct units, e.g. 280ns, 1us, 3MHz, 10%.
    ''')
    parser.add_argument('--t0h', nargs=2, required=True, help='High time for transmitting a 0 (min, max)')
    parser.add_argument('--t0l', nargs=2, required=True, help='Low time for transmitting a 0 (min, max)')
    parser.add_argument('--t1h', nargs=2, required=True, help='High time for transmitting a 1 (min, max)')
    parser.add_argument('--t1l', nargs=2, required=True, help='Low time for transmitting a 1 (min, max)')
    parser.add_argument('-b', '--baudrate', '-f', '--frequency', required=True, help='SPI frequency')
    parser.add_argument('-m', '--margin', default='0', help='Error margin (percent or ratio)')
    parser.add_argument('-n', '--n-best', type=int, default=3, help='Number of best results to show')
    args = parser.parse_args(args)

    t = lambda s: convert_unit(s, TIME_UNITS)
    times = TimeRanges(
        # t0h=(220e-9, 380e-9),
        # t0l=(580e-9, 1e-6),
        # t1h=(580e-9, 1e-6),
        # t1l=(580e-9, 1e-6),
        t0h=(t(args.t0h[0]), t(args.t0h[1])),
        t0l=(t(args.t0l[0]), t(args.t0l[1])),
        t1h=(t(args.t1h[0]), t(args.t1h[1])),
        t1l=(t(args.t1l[0]), t(args.t1l[1])),
    )

    margin = convert_unit(args.margin, PERCENT_UNITS)
    if margin != 0:
        times = times.with_margin(margin)

    baudrate = convert_unit(args.baudrate, FREQ_UNITS)

    print(f'Using:\n  {margin=}\n  {baudrate=}\n  {times=}')

    scored = times.to_bits(baudrate).find_scores()[:args.n_best]

    print('Found (score, bits):')
    for score, bits in scored:
        print(f'  {score:3}: {bits} (0: {bits.n0()}, 1: {bits.n1()})')


if __name__ == "__main__":
    main()
