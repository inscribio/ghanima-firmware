import sigrokdecode as srd
from collections import deque

class SamplerateError(Exception):
    pass

def normalize_time(t):
    if abs(t) >= 1.0:
        return '%.3f s  (%.3f Hz)' % (t, (1/t))
    elif abs(t) >= 0.001:
        if 1/t/1000 < 1:
            return '%.3f ms (%.3f Hz)' % (t * 1000.0, (1/t))
        else:
            return '%.3f ms (%.3f kHz)' % (t * 1000.0, (1/t)/1000)
    elif abs(t) >= 0.000001:
        if 1/t/1000/1000 < 1:
            return '%.3f μs (%.3f kHz)' % (t * 1000.0 * 1000.0, (1/t)/1000)
        else:
            return '%.3f μs (%.3f MHz)' % (t * 1000.0 * 1000.0, (1/t)/1000/1000)
    elif abs(t) >= 0.000000001:
        if 1/t/1000/1000/1000:
            return '%.3f ns (%.3f MHz)' % (t * 1000.0 * 1000.0 * 1000.0, (1/t)/1000/1000)
        else:
            return '%.3f ns (%.3f GHz)' % (t * 1000.0 * 1000.0 * 1000.0, (1/t)/1000/1000/1000)
    else:
        return '%f' % t

class Decoder(srd.Decoder):
    api_version = 3
    id = 'tasks-timing'
    name = 'TasksTiming'
    longname = 'Tasks time averaging'
    desc = 'Calculate time of high pulses'
    license = 'gplv2+'
    inputs = ['logic']
    outputs = []
    tags = ['Clock/timing', 'Util']
    channels = (
        {'id': 'data', 'name': 'Data', 'desc': 'Data line'},
    )
    annotations = (
        ('time', 'Time'),
        ('average', 'Average'),
        ('count', 'Count'),
        ('min', 'Min'),
        ('max', 'Max'),
        ('stdev', 'StDev'),
    )
    annotation_rows = (
        ('time', 'Time', (0,)),
        ('average', 'Average', (1,)),
        ('count', 'Count', (2,)),
        ('min', 'Min', (3,)),
        ('max', 'Max', (4,)),
        ('stdev', 'StDev', (5,)),
    )
    options = (
        {'id': 'avg_period', 'desc': 'Averaging period', 'default': 100},
        {'id': 'show_count', 'desc': 'Show count', 'default': 'no', 'values': ('yes', 'no')},
        {'id': 'show_other', 'desc': 'Show other stats', 'default': 'no', 'values': ('yes', 'no')},
    )

    def __init__(self):
        self.reset()

    def reset(self):
        self.samplerate = None
        self.last_n = deque()
        self.chunks = 0
        self.level_changed = False
        self.last_sample0 = None
        self.last_t = None
        self.count = 0

    def metadata(self, key, value):
        if key == srd.SRD_CONF_SAMPLERATE:
            self.samplerate = value

    def start(self):
        self.out_ann = self.register(srd.OUTPUT_ANN)

    def decode(self):
        if not self.samplerate:
            raise SamplerateError('Cannot decode without samplerate.')
        while True:
            self.wait({0: 'r'})
            sample0 = self.samplenum

            if self.last_sample0 is not None and self.last_t is not None:
                mean = None
                self.put(self.last_sample0, sample0, self.out_ann,
                         [0, [normalize_time(self.last_t)]])

                if self.options['avg_period'] > 0 and len(self.last_n) > 0:
                    mean = sum(self.last_n) / len(self.last_n)
                    self.put(self.last_sample0, sample0, self.out_ann,
                             [1, [normalize_time(mean)]])

                if self.options['show_count'] == 'yes':
                    self.put(self.last_sample0, sample0, self.out_ann,
                             [2, [str(self.count)]])

                if self.options['show_other'] == 'yes':
                    self.put(self.last_sample0, sample0, self.out_ann,
                             [3, [normalize_time(min(self.last_n))]])

                    self.put(self.last_sample0, sample0, self.out_ann,
                             [4, [normalize_time(max(self.last_n))]])

                    if mean is not None and len(self.last_n) > 1:
                        std = 1 / (len(self.last_n) - 1) * sum((t - mean)**2 for t in self.last_n)
                        self.put(self.last_sample0, sample0, self.out_ann,
                                 [5, [normalize_time(std)]])

            self.wait({0: 'f'})
            sample1 = self.samplenum

            samples = sample1 - sample0
            t = samples / self.samplerate

            if t > 0:
                self.last_n.append(t)
            if len(self.last_n) > self.options['avg_period']:
                self.last_n.popleft()

            self.last_sample0 = sample0
            self.last_t = t
            self.count += 1
