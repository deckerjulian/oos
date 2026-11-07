# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# Reference model of the AFE setting path, see docs/protocols/afe-settings.md.
# Default: the filters of the unit; --stock: Rigol 1.04 for comparison.
# Models the per-channel shadow registers and prints the AFE write sequence (plus gear ioctl and ADC words).
import os, struct, sys
# calibration data of a unit after a self-cal: an older copy can have block group 3 of cal_vertical empty
CAL = os.environ.get('OOS_CAL_DIR', os.path.join(os.path.dirname(os.path.abspath(__file__)), '../local/cal')) + '/'
def rdcal(name, fmt, n):
    b = open(CAL + name, 'rb').read(); return struct.unpack_from('<%d%s' % (n, fmt), b, 28)
LSB = rdcal('cal_lsb.hex', 'd', 16)                       # [ch*4 + inputgain]  V per DAC code
ZERO = rdcal('cal_afe_zero.hex', 'i', 80)                 # [ch*20 + i]
BW = list(rdcal('cal_afe_bandwidth.hex', 'i', 80))        # [ch*20 + i]
for ch in range(4): BW[ch*20+4], BW[ch*20+10], BW[ch*20+16] = 110, 97, 97   # fixed values that replace these entries of the file
_v = open(CAL + 'cal_vertical.hex', 'rb').read()
def vrec(var, ch, idx): return struct.unpack_from('<q8idii', _v, 28 + ((var*4 + ch)*801 + idx)*56)
KEYS = [vrec(0, 0, i)[0] for i in range(801)]
FILT = [(0, 0, 5), (0, 0, 23), (1, 0, 23), (2, 0, 23), (3, 3, 63)]   # same for all channels

# bandwidth name -> value
BW_VALUE = {'FULL': 0x0, 'OFF': 0x1, 'GND': 0x11, 'S20M': 0x2, 'Auto': 0x12, '8M': 0x100010, '125M': 0x7, '155M': 0x5,
         '315M': 0x6, '350M': 0xa, '1G': 0x303140, '1.1G': 0x303160, '1.2G': 0x303180}
for mhz in range(5, 315, 5):
    if '%dM' % mhz not in BW_VALUE: BW_VALUE['%dM' % mhz] = (mhz // 5) << 4
for mhz in range(400, 950, 50): BW_VALUE['%dM' % mhz] = 0x303000 | (mhz // 50) << 4
LAST_AUTO_C = 63             # last auto filter code (initial 63 = 1.25 GSa/s)

def bw_auto_c(fs): return max(1, int(fs) // 19841269)        # Fs / (1.25e9 / 63), at least 1

def bw_filter(v, fs=1.25e9):
    """Bandwidth value -> Hz filter (a, b, c)"""
    special = {1: (3, 3, 63), 2: (0, 0, 5), 0x11: (0, 0, 0), 5: (0, 0, 31), 6: (0, 0, 63), 7: (0, 0, 25), 0xa: (3, 3, 7)}
    if v in special: return special[v]
    if v == 0x12: return (0, 0, bw_auto_c(fs))
    return ((v >> 20) & 0xff, (v >> 12) & 0xff, (v >> 4) & 0xff)

def bw_code(v, auto_c=None):
    """the variant decision"""
    if v == 1: return 0xa
    if v in (2, 5, 6): return v
    if v == 7: return 5
    if v & 0xf:
        if v != 0x12: return 2
        c = LAST_AUTO_C if auto_c is None else auto_c
        return 2 if c <= 0xf else 5 if c <= 0x2a else 6
    return 2 if v <= 0xf0 else 5 if v <= 0x2a0 else 6 if v <= 0x3f0 else 0xa

def bw_variant(v, auto_c=None):
    c = bw_code(v, auto_c)
    return {2: 0, 5: 1, 7: 1, 0xa: 3, 6: 2}.get(c, 1)    # code 0xa -> block group 3, no hardware-version check

class Afe:
    def __init__(s, ch, log):
        s.ch, s.log = ch, log
        # shadows after the AFE init + efuse read (0x2f..0x34 left out: not touched by setters except 0x2f/0x32)
        s.r = {0x06: 0x1d00, 0x07: 0x8440, 0x20: 0x0008, 0x21: 0x0a91, 0x22: 0x5a00, 0x23: 0x1000, 0x24: 0, 0x25: 0,
               0x26: 0x3200, 0x27: 0x80c4, 0x28: 0x8000, 0x29: 0x0282, 0x2a: 0x3c44, 0x2b: 0x4c5f, 0x2c: 0x000c,
               0x2d: 0, 0x2e: 0x1ff9, 0x35: 0, 0x36: 0, 0x37: 0xc000}
    def w(s, reg, val=None):
        if val is not None: s.r[reg] = val & 0xffff
        s.log.append((s.ch, reg, s.r[reg]))
    def bf(s, reg, lsb, width, v):
        m = ((1 << width) - 1) << lsb; s.r[reg] = (s.r[reg] & ~m & 0xffff) | ((v << lsb) & m)
    # setter primitives
    def hz_enable(s, en):
        mode = s.r[0x07] & 0x4000          # (bit13 path not modelled: never set in normal use)
        if en:
            if mode == 0x4000:
                s.bf(0x23, 11, 3, 5); s.w(0x23); s.r[0x29] &= ~0x300; s.w(0x29); s.r[0x2a] &= 0x0fff; s.w(0x2a)
            else:
                s.bf(0x23, 11, 3, 2); s.w(0x23); s.bf(0x29, 8, 2, 2); s.w(0x29); s.r[0x2a] = (s.r[0x2a] & 0xfff) | 0x2000; s.w(0x2a)
        else:
            s.r[0x23] &= ~0x3800; s.w(0x23); s.r[0x29] |= 0x300; s.w(0x29); s.r[0x2a] |= 0xc000; s.w(0x2a)
    def hz_input_gain(s, g):
        if g == 0:
            s.bf(0x07, 13, 2, 2); s.w(0x07); s.bf(0x23, 11, 3, 5); s.w(0x23); return
        s.bf(0x07, 13, 2, 0); s.w(0x07)
        if g == 1: s.r[0x20] = (s.r[0x20] & 0x1ff8) | 0xa000
        else: s.bf(0x20, 0, 3, 4 if g == 2 else 6)
        s.bf(0x23, 11, 3, 2); s.w(0x20); s.w(0x23)
    def hz_vga(s, v):
        if v < 2: s.bf(0x07, 13, 2, 2); s.w(0x07); s.bf(0x29, 8, 2, v)
        elif v == 2: s.bf(0x07, 13, 2, 0); s.w(0x07); s.bf(0x29, 8, 2, 2)
        else: s.r[0x29] |= 0x300
        s.w(0x29)
    def hz_att(s, v): s.bf(0x29, 6, 2, v); s.w(0x29)
    def hz_sga(s, v):
        s.bf(0x07, 13, 2, 0 if v >= 2 else 2); s.w(0x07); s.bf(0x2a, 12, 2, v); s.w(0x2a)
    def hz_sgar(s, v): s.bf(0x2a, 14, 2, v); s.w(0x2a)
    def mux(s, v): s.bf(0x2e, 11, 3, v); s.w(0x2e)
    def mux_r(s, v): s.bf(0x2e, 14, 2, v); s.w(0x2e)
    def hz_output(s, which, en): s.bf(0x2b, 10 if which == 0 else 11, 1, en); s.w(0x2b)
    def coupling(s, dc): s.bf(0x07, 15, 1, dc); s.w(0x07)
    def dac(s, code): s.w(0x28, code)
    def hz_filter(s, a, b, c): s.r[0x2e] = (s.r[0x2e] & 0xf900) | (a & 3) << 6 | (b & 3) << 9 | (c & 0x3f); s.w(0x2e)
    def zero_first(s, mode, a, b):
        reg = 0x23 if mode == 0x4000 else 0x26
        s.r[reg] = (s.r[reg] & 0xf800) | (a & 1) << 10 | (b & 0x3ff); s.w(reg)
    def zero_vga(s, val, sel):
        if sel: s.bf(0x35, 0, 9, val); s.w(0x35)
        else:
            s.r[0x29] = (s.r[0x29] & 0x3ff) | ((val << 7) & 0xfc00); s.bf(0x35, 9, 3, val & 7); s.w(0x29); s.w(0x35)
    def zero_sga(s, mode, a, b):   # modes 0 / 0x4000 -> reg 0x27 (0x2000/0x6000 -> 0x2d, unused here)
        s.r[0x27] = (s.r[0x27] & 0x8080) | (a & 0x7f) << 8 | (b & 0x7f); s.w(0x27)
    def zero_mux(s, a, b): s.r[0x06] = (s.r[0x06] & 0x8080) | (b & 0x7f) << 8 | (a & 0x7f); s.w(0x06)
    def bw_trim(s, sel, v):
        t = {1: (0x22, 8, 8), 6: (0x20, 4, 3), 7: (0x21, 7, 5), 8: (0x21, 0, 7), 9: (0x21, 12, 4), 10: (0x24, 0, 10), 11: (0x25, 0, 10)}[sel]
        s.bf(t[0], t[1], t[2], v); s.w(t[0])

def variant(bl, hw=0):
    if bl == 2: return 0
    if bl in (5, 7): return 1
    if (hw | 8) == 0xc and bl == 10: return 3
    if bl in (6, 10): return 2
    return 1
def scale_index(nv):
    for i, k in enumerate(KEYS):
        if k >= nv: return i
    return 800

def block_ok(var, ch): return vrec(var, ch, 0)[0] != 0

def set_scale(a, scale_nv, var, fixed_trim=False):
    ch = a.ch; i = scale_index(scale_nv); r = vrec(var, ch, i)
    ig, vga, att, sga, sgar, mux, muxr = r[1:8]
    a.hz_enable(1)
    a.hz_input_gain(ig); a.hz_vga(vga); a.hz_att(att); a.hz_sga(sga); a.hz_sgar(sgar)
    a.mux(mux); a.mux_r(muxr); a.hz_output(0, 1); a.hz_output(1, 0)
    a.log.append('ioctl(/dev/dac_gear_change, 0x7801, &(u8)0x%02x)  # CH%d gear 1 -> GPIO high' % (ch | 0x10, ch + 1))
    z = ZERO[ch*20:ch*20+20]
    if 1 <= ig <= 3:
        if vga == 2: a.zero_first(0, z[14], z[15]); a.zero_sga(0, z[16], z[17]); a.zero_mux(z[18], z[19])
    elif ig == 0:
        if vga == 0: a.zero_first(0x4000, z[0], z[1]); a.zero_vga(z[2], 1); a.zero_sga(0x4000, z[3], z[4]); a.zero_mux(z[5], z[6])
        elif vga == 1: a.zero_first(0x4000, z[7], z[8]); a.zero_vga(z[9], 0); a.zero_sga(0x4000, z[10], z[11]); a.zero_mux(z[12], z[13])
    b = BW[ch*20:ch*20+20]
    if 1 <= ig <= 3:
        o = (ig*0x18 - 0x14)//4
        vals = [b[o+k] for k in range(5)]
        if fixed_trim: vals[2], vals[3] = 17, 126           # bandwidth trims: r21[11:7]=17, r21[6:0]=126 fixed
        for sel, v in zip((1, 6, 7, 8, 9), vals): a.bw_trim(sel, v)
    elif ig == 0:
        a.bw_trim(10, b[18]); a.bw_trim(11, b[19])
    return r

def set_offset(a, r, offset_nv, position_nv=0, invert=False):
    ig, zero = r[1], r[10]; lsb = LSB[a.ch*4 + ig]; sign = -1 if invert else 1
    code = int(-(sign*offset_nv + position_nv)/1e9/lsb + zero) if lsb != 0 else zero
    a.dac(code); return code

def usage():
    print('usage: afesim.py [--stock] [--fs SA/S] <ch 1-4> <V/div> <offset V> <bandwidth>\n'
          '  default: bandwidth = enum label (OFF, GND, Auto, S20M, 5M..310M, 315M, 350M, 400M..900M, 1G, 1.1G, 1.2G, FULL)\n'
          '                or a raw driver value (0x...)\n'
          '  --stock:      bandwidth = driver enum 2 (20M), 5 (70M), 6 (100M), 7 (125M), 10 (250M = OFF on the DHO924S)')
    sys.exit(1)

if __name__ == '__main__':
    args = sys.argv[1:]; stock = '--stock' in args; fs = 1.25e9
    if stock: args.remove('--stock')
    if '--fs' in args: k = args.index('--fs'); fs = float(args[k+1]); del args[k:k+2]
    if len(args) != 4: usage()
    ch = int(args[0]) - 1; scale = float(args[1]); off = float(args[2]); bwarg = args[3]
    log = []
    a = Afe(ch, log)
    log.append('ADC 0x1034 <- 0x01545251, 0x0155000c  (ADC regs 0x54 <- 0x4210, 0x55 <- 0x0008)')
    if stock:
        bl = int(bwarg, 0); f = FILT[{2: 0, 6: 2, 7: 3, 10: 4}.get(bl, 1)]; var = variant(bl)
        head = '# stock 1.04: bandwidth %d -> Hz filter %s, vertical block group %d' % (bl, f, var)
    else:
        bl = BW_VALUE[bwarg] if bwarg in BW_VALUE else int(bwarg, 0)
        f = bw_filter(bl, fs); auto_c = bw_auto_c(fs) if bl == 0x12 else None
        var = bw_variant(bl, auto_c)
        head = '# bandwidth %s = 0x%x -> Hz filter %s (masked a&3, b&3, c&0x3f), vertical block group %d' % (bwarg, bl, f, var)
        if not block_ok(var, ch):
            head += '\n# WARNING: block group %d of CH%d is empty in this cal_vertical.hex (all-zero gains)' % (var, ch+1)
    print(head)
    a.hz_filter(*f)
    r = set_scale(a, int(scale*1e9), var, fixed_trim=not stock)
    code = set_offset(a, r, int(off*1e9))
    print('# CH%d %g V/div, offset %g V: record %s' % (ch+1, scale, off, r))
    for e in log: print('  ' + e if isinstance(e, str) else '  ch%d w 0x%02x <- 0x%04x' % (e[0]+1, e[1], e[2]))
    print('# DAC code %d, FPGA digital gain %.5f' % (code, r[9]))
