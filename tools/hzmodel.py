# Copyright (C) 2026 Julian Decker
# SPDX-License-Identifier: GPL-2.0-or-later
# model of the horizontal parameters, see docs/protocols/horizontal-config.md
RATIO={m:(1 if m in(0,1,2,4,8) else 2 if m in(3,5,6,9,10,12) else 4) for m in range(16)}
FS=10**15
def pow2(n):
    v=n-1; bits=1
    if v>=2:
        while True:
            bits+=1; c=v>3; v>>=1
            if not c: break
    return 1<<bits
def memsize(pts,mode=0):
    if mode==2: n=((pts+0x17f)//320+2)*32*2
    else: n=(pts+0x180)//10
    n=max(n,32) if n>32 else 32
    # average (fork spec 01, 1.3): from 25 M points on, the next multiple of 2048 bursts above
    n=(n//2048+1)*2048 if mode==2 and pts>=25000000 else pow2(n)
    return min(n,(16777216-0x100000)//2)
def max_frames_avg(seg):
    # live ring in average mode (fork spec 01, 1.3): the half ring with the average segment
    return (16777216//2-0x100000)//max(seg,1)
def avg_vectors():
    # test vectors for hal hz::mem_size_avg / max_frames_avg: points per channel, ratio, segment, frames
    for pts in (0,1000,1408,12500,100000,1000000,2500000,6250000,25000000,25015297,31250000,125000000):
        for ratio in (1,2,4):
            seg=memsize(pts*ratio,2)
            print(pts,ratio,seg,max_frames_avg(seg))
def max_record_frames(seg):
    # segmented record (fork spec 03, 1.5): the whole stock frame memory less the reserved 1 M bursts
    return (16777216-0x100000)//max(seg,1)
def maxauto(cnt,la=False):
    if la: return 12500000 if cnt==2 else (10000000 if cnt>2 else 31250000)
    return 62500000 if cnt==2 else (31250000 if cnt>2 else 125000000)
TABLE={2500:2000,200000:125000,10**6:500000,2*10**6:1250000,5*10**6:2500000,10**7:6250000,2*10**7:12500000,25*10**6:12500000,5*10**7:31250000,10**8:62500000,125*10**6:62500000,2*10**8:156250000,25*10**7:156250000,4*10**8:312500000,5*10**8:312500000,10**9:625000000}
def cisfloat(a,b,c):
    from fractions import Fraction
    f=Fraction(a*b,c)
    if f.denominator==1: return f.numerator
    import math
    x=math.ceil(f); return (x+1)&~1
def prep(mask,tb,depth=0,offset=0,stock=False):
    ratio=RATIO[mask]; mr=1250000000//ratio; cnt=bin(mask).count('1')
    if depth:
        memsel=depth; rate=min(depth*FS//tb//10,mr)
    else:
        pts=cisfloat(10*tb,mr,FS); amax=10**6 if stock else maxauto(cnt)
        if pts>amax: memsel=amax; rate=amax*FS//tb//10
        else: memsel=pts; rate=mr
    if rate in TABLE:
        rate=TABLE[rate]
        if not depth: memsel=10*tb*rate//FS
    dot=FS//rate
    seg=memsize(memsel*ratio)
    max_frames=(16777216//2-0x100000)//seg
    memstart=offset-dot*memsel//2
    post=-(-(memsel*dot+memstart)//dot); pre=max(0,memsel-post)
    if rate>=62500001: r10d0=0x8000|((mr//rate)&0xff); r10d8=1
    else: r10d0=0; r10d8=62500000//rate
    return dict(ratio=ratio,rate=rate,memsel=memsel,dot_fs=dot,seg_10e0=hex(seg),max_frames=max_frames,max_record_frames=max_record_frames(seg),pre_4050=pre,post_4054=post,r10d0=hex(r10d0),r10d8=r10d8,r10e4=hex(memsel),r1148=640//ratio+32)
if __name__=='__main__':
    import sys
    if sys.argv[1:]==['avg']:
        avg_vectors(); sys.exit()
    print(prep(1,2*10**11)); print(prep(3,10**9))
    for tb in [10**6*x for x in (1,2,5,10,20,50,100,200,500)]+[10**9*x for x in (1,2,5,10,20,50,100,200,500)]+[10**12*x for x in (1,2,5,10,20)]:
        a=prep(1,tb); b=prep(3,tb); c=prep(15,tb); s=prep(1,tb,stock=True)
        print(tb/1e15, a['rate'],a['memsel'],a['seg_10e0'], '|2ch',b['rate'],b['memsel'],'|4ch',c['rate'],c['memsel'],'|stock1ch',s['rate'],s['memsel'])
