#!/usr/bin/env python3
# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
"""ADC channel-mode model: the register writes needed when the set of
enabled analog channels changes, computed from the unit's /rigol/data/cal_adc.hex.

Source: docs/protocols/adc-mode.md. No vendor code; behaviour only.

All ADC writes go to FPGA register 0x1034 (ADC SPI master: bit 24 start, bits 22:16 ADC register, 15:0 data).
The model keeps software shadows (several ADC registers are read-modify-write of a shadow), so a
sequence of modes must be run in order, starting from the state after boot.

usage:
  adcmodel.py                      boot + CH1 -> CH1+CH2 -> CH1..CH4 (default demo)
  adcmodel.py 1 3 f                the same, masks in hex (bit0 = CH1 .. bit3 = CH4)
  adcmodel.py --dump               decoded cal_adc.hex
  adcmodel.py --cal DIR ...        calibration directory (default local/cal, or $OOS_CAL_DIR)
  adcmodel.py --no-boot 3          only the mode change, shadows as after boot (mode 0 applied)
  adcmodel.py --stock ...          stock 1.04 delays
  adcmodel.py --wpu ...            also print the 0x105c digital-gain writes of the read path (all channels x1)
"""
import os, struct, sys, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_CAL = os.environ.get('OOS_CAL_DIR', os.path.join(HERE, '..', 'local', 'cal'))

# mask (1..15) -> ADC mode
MODE_OF_MASK = [None, 0, 1, 4, 2, 5, 7, 10, 3, 6, 8, 10, 9, 10, 10, 10]
MODE_NAME = ['CH1', 'CH2', 'CH3', 'CH4', 'CH1+CH2', 'CH1+CH3', 'CH1+CH4', 'CH2+CH3', 'CH2+CH4', 'CH3+CH4', '4ch']
# mode -> (i, j) channel pair for the SPU fine gain, modes 0..9
FINE_PAIR = [(0, 0), (1, 1), (2, 2), (3, 3), (0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
# ratio (interleave factor) per mask
RATIO_OF_MASK = [1, 1, 1, 2, 1, 2, 2, 4, 1, 2, 2, 4, 2, 4, 4, 4]

# input mode: mode -> (SWONP1, SWONP2) shadows.
# reg 0x54 word data = SWONP1 ^ 0x1041, reg 0x55 data = SWONP2 ^ 0x0004.
SWONP = {0: (0x8421, 0), 1: (0x0842, 1), 2: (0x2108, 4), 3: (0x1084, 2),
         4: (0x0821, 1), 5: (0x2021, 4), 6: (0x1021, 2), 7: (0x2042, 4), 8: (0x1042, 2), 9: (0x1108, 2),
         10: (0x1041, 4)}

REC = 0x68           # bytes per mode record in the payload
FINE = 0x478         # 4 blocks of 16 x int32 (2-bit SPU fine gain per core slot), one per channel


def load_container(path):
    """/rigol/data/cal_*.hex container: 20-byte header, u32 payload length, u32 crc32(payload), payload."""
    d = open(path, 'rb').read()
    ln, crc = struct.unpack_from('<II', d, 20)
    pay = d[28:28 + ln]
    if len(pay) != ln or (zlib.crc32(pay) & 0xffffffff) != crc:
        raise ValueError(f'{path}: bad length or crc')
    return pay


class CalAdc:
    def __init__(self, caldir):
        p = load_container(os.path.join(caldir, 'cal_adc.hex'))
        if len(p) != 0x738:
            raise ValueError(f'cal_adc.hex payload {len(p)} bytes, expected 1848')
        self.p = p

    def s16(self, off):
        return struct.unpack_from('<h', self.p, off)[0]

    def i32(self, off):
        return struct.unpack_from('<i', self.p, off)[0]

    # --- field locations of the ADC parameters (file offsets in the payload) ---
    def offset_src(self, mode, core):            # core offset: record[mode] + 2*core
        return mode * REC + 2 * core

    def gain_src(self, mode, core):               # core gain
        if mode <= 3:
            ch = mode
        elif mode == 10:
            ch = core % 4
        else:
            lo, hi = FINE_PAIR[mode]
            ch = lo if core % 2 == 0 else hi
        off = ch * REC + 0x20 + 2 * core
        if mode == 9 and core in (3, 7):          # stock quirk: reads record 4 instead of record 3
            off = 4 * REC + 0x20 + 2 * core
        return off

    def dly_src(self, mode, i):                   # sample-clock delay i = 0..3
        return (mode if mode <= 3 else 0) * REC + 0x40 + 2 * i

    def clk_src(self, mode):                      # clock delay: (a, b) offsets
        r = 4 + mode if mode <= 3 else 4
        return r * REC + 0x42, r * REC + 0x40

    def ref_src(self):                            # reference of ADC 0 (that of ADC 1 reads 0x4a, never written)
        return 0x48

    def fine_buf(self, mode):                     # 16 x int32 of the SPU fine gain
        if mode < 10:
            i, j = FINE_PAIR[mode]
            return [self.i32(FINE + 64 * i + 4 * k) for k in range(8)] + \
                   [self.i32(FINE + 64 * j + 4 * k) for k in range(8, 16)]
        return [self.i32(FINE + 64 * (k // 4) + 4 * k) for k in range(16)]


class Adc:
    """Software shadows + the write list."""
    def __init__(self, cal, stock=False):
        self.cal = cal
        self.stock = stock
        # start values
        self.sh = {'r01': 0x0000, 'r04': 0x8000, 'r05': 0x0000, 'r09': 0x5020,
                   'gain': [0x0002] * 8, 'off': [0x0000] * 8, 'swonp1': 0x8421, 'swonp2': 0}
        self.w = []

    def wr(self, reg, val, pre, post, what):
        self.w.append((reg, val & 0xffffffff, pre, post, what))

    def spi(self, addr, data, pre, post, what):
        self.wr(0x1034, 0x01000000 | (addr & 0x7f) << 16 | (data & 0xffff), pre, post, what)

    # input mode words
    def input_mode(self, mode):
        a, b = SWONP[mode]
        self.sh['swonp1'], self.sh['swonp2'] = a, b
        if self.stock:
            self.spi(0x54, a ^ 0x1041, 100, 100, f'input mode {mode} ({MODE_NAME[mode]}): reg 0x54')
            self.spi(0x55, b ^ 0x0004, 100, 100, 'reg 0x55')
        else:
            self.spi(0x54, a ^ 0x1041, 100, 0, f'input mode {mode} ({MODE_NAME[mode]}): reg 0x54')
            self.spi(0x55, b ^ 0x0004, 100, 100, 'reg 0x55')

    # ADC parameters of a mode
    def adc_parameter(self, mode):
        c = self.cal
        # reference of ADC 0: reg 0x09 bits 5:0, word data = shadow ^ 0x2720; that of ADC 1 updates the shadow only
        v0, v1 = c.s16(0x48), c.s16(0x4a)
        self.sh['r09'] = (self.sh['r09'] & 0xffc0) | (v0 & 0x3f)
        self.spi(0x09, self.sh['r09'] ^ 0x2720, 100, 100, f'ref {v0} (cal 0x048)')
        self.sh['r09'] = (self.sh['r09'] & 0xffc0) | (v1 & 0x3f)
        # SPU fine gain 0x1058: 16 x 2-bit fields
        buf = c.fine_buf(mode)
        v = 0
        for k, f in enumerate(buf):
            v |= (f & 3) << (2 * k)
        self.wr(0x1058, v, 0, 0, f'SPU fine gain {buf[:8]}+{buf[8:]}')
        # per-core gain, ADC 0 only (ADC 1 only touches the shared shadow)
        order = [(0, 4), (1, 5), (2, 6), (3, 7)] if mode in (0, 1, 2, 3, 10) else [(0, 4), (2, 6), (1, 5), (3, 7)]
        for pair in order:
            for core in pair:
                off = c.gain_src(mode, core)
                g = c.s16(off)
                if not -64 <= g <= 63:            # out of range: no write, shadow unchanged
                    continue
                self.sh['gain'][core] = (self.sh['gain'][core] & 0xff80) | (g & 0x7f)
                pre, post = (100, 100) if self.stock else (0, 80)
                self.spi(0x15 + 8 * core, self.sh['gain'][core], pre, post, f'core {core} gain {g} (cal {off:#05x})')
            for core in pair:                     # ADC 1 value: shadow only
                g1 = c.s16(c.gain_src(mode, core) + 0x10)
                if -64 <= g1 <= 63:
                    self.sh['gain'][core] = (self.sh['gain'][core] & 0xff80) | (g1 & 0x7f)
        # per-core offset, ADC 0 cores 0..7 then ADC 1 (shadow only)
        for core in range(8):
            off = c.offset_src(mode, core)
            o = c.s16(off)
            if not -1024 <= o <= 1023:            # out of range: no write
                continue
            self.sh['off'][core] = (self.sh['off'][core] & 0xf800) | (o & 0x7ff)
            self.spi(0x14 + 8 * core, self.sh['off'][core], 100, 100, f'core {core} offset {o} (cal {off:#05x})')
        for core in range(8):
            o1 = c.s16(c.offset_src(mode, core) + 0x10)
            if -1024 <= o1 <= 1023:
                self.sh['off'][core] = (self.sh['off'][core] & 0xf800) | (o1 & 0x7ff)
        # sample-clock delays: reg 0x04 low/high byte (i = 0, 1), reg 0x05 low/high byte (i = 2, 3), data ^ 0x8080
        for i in range(4):
            off = c.dly_src(mode, i)
            d = c.s16(off) & 0xff
            reg = 'r04' if i < 2 else 'r05'
            s = self.sh[reg]
            s = (s & 0xff00) | d if i % 2 == 0 else (s & 0x00ff) | d << 8
            self.sh[reg] = s
            pre, post = (100, 100) if self.stock else (0, 100)
            self.spi(0x04 if i < 2 else 0x05, s ^ 0x8080, pre, post, f'dly smp clk {i} = {d} (cal {off:#05x})')
        # clock delay: reg 0x01 = (shadow & 0xe000) | (a & 7) << 10 | (b & 0x3ff)
        oa, ob = c.clk_src(mode)
        a, b = c.s16(oa), c.s16(ob)
        self.sh['r01'] = (self.sh['r01'] & 0xe000) | (a & 7) << 10 | (b & 0x3ff)
        pre, post = (100, 100) if self.stock else (0, 100)
        self.spi(0x01, self.sh['r01'], pre, post, f'clock delay a={a} b={b} (cal {oa:#05x}/{ob:#05x})')

    # input mode + parameters
    def channel_mode(self, mode):
        self.input_mode(mode)
        self.adc_parameter(mode)

    # the restore after a band-limit change
    def adc_onoff(self):
        self.spi(0x54, self.sh['swonp1'] ^ 0x1041, 100, 0 if not self.stock else 100, 'restore reg 0x54')
        self.spi(0x55, self.sh['swonp2'] ^ 0x0004, 100, 100, 'restore reg 0x55')


def wpu_gain(mask, gains, sh=0x5555):
    """Read-path gain per enabled channel. gains[ch] = 1 or 2. Returns the list of 0x105c values written (channel order ascending [A])."""
    r = RATIO_OF_MASK[mask]
    out = []
    for ch in range(4):
        if not mask >> ch & 1:
            continue
        g = gains[ch] & 3
        if r == 1:
            sh = 0
            for k in range(8):
                sh |= g << (2 * k)
        elif r == 2:
            lower = not any(mask >> c & 1 for c in range(ch))     # lower-numbered channel of the pair
            byte = g * 0x55
            sh = (sh & 0xff00) | byte if lower else (sh & 0x00ff) | byte << 8
        else:
            fields = {0: (0, 2), 1: (1, 3), 2: (4, 6), 3: (5, 7)}[ch]
            for k in fields:
                sh = (sh & ~(3 << 2 * k)) | g << (2 * k)
        out.append((ch, sh & 0xffff))
    return out


def dump(cal):
    p = cal
    print('cal_adc.hex payload, 11 records of 0x68 bytes (s16) + SPU fine-gain blocks')
    for m in range(11):
        b = m * REC
        row = lambda o, n: ' '.join(f'{p.s16(b + o + 2 * k):4d}' for k in range(n))
        print(f'rec {m:2d} @{b:#05x} off0[8] {row(0, 8)} | off1[8] {row(0x10, 8)}')
        print(f'            gain0[8] {row(0x20, 8)} | gain1[8] {row(0x30, 8)} | +40[4] {row(0x40, 4)} | +48[2] {row(0x48, 2)}')
    for ch in range(4):
        print(f'fine gain CH{ch + 1} @{FINE + 64 * ch:#05x}:', [p.i32(FINE + 64 * ch + 4 * k) for k in range(16)])
    tail = p.p[FINE + 256:]
    print(f'payload {FINE + 256:#05x}..{len(p.p):#05x}: {"all zero" if not any(tail) else "NOT zero"}')


def main(argv):
    caldir, stock, boot, wpu, masks, do_dump = DEFAULT_CAL, False, True, False, [], False
    it = iter(argv)
    for a in it:
        if a == '--cal': caldir = next(it)
        elif a == '--stock': stock = True
        elif a == '--no-boot': boot = False
        elif a == '--wpu': wpu = True
        elif a == '--dump': do_dump = True
        elif a in ('-h', '--help'): print(__doc__); return 0
        else: masks.append(int(a, 16))
    cal = CalAdc(caldir)
    if do_dump:
        dump(cal); return 0
    if not masks:
        masks = [0x1, 0x3, 0xf]
    adc = Adc(cal, stock)
    adc.adc_parameter(0)                          # ADC parameters of mode 0 (after the init list)
    if boot:
        print('# boot: ADC parameters of mode 0 (input mode 0 is already set by the ADC init list)')
        for w in adc.w: print(fmt(w))
    adc.w = []
    last = 0 if boot else 1                       # stored mask: 0 after start [A]
    for m in masks:
        if not 1 <= m <= 15:
            print(f'# mask {m:#x}: no channel -> nothing written'); continue
        if m == last:
            print(f'# mask {m:#x}: unchanged -> nothing written'); continue
        mode = MODE_OF_MASK[m]
        print(f'# mask {m:#x} -> ADC mode {mode} ({MODE_NAME[mode]}), ratio {RATIO_OF_MASK[m]}')
        adc.channel_mode(mode)
        for w in adc.w: print(fmt(w))
        adc.w = []
        if wpu:
            for ch, v in wpu_gain(m, [1, 1, 1, 1]):
                print(fmt((0x105c, v, 0, 0, f'read-path ADC gain CH{ch + 1} x1')))
        last = m
    return 0


def fmt(w):
    reg, val, pre, post, what = w
    d = (f'sleep {pre:3d} us, ' if pre else ' ' * 15) + f'{reg:#06x} <- {val:#010x}' + (f', sleep {post} us' if post else '')
    return f'{d:58s} ; {what}'


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
