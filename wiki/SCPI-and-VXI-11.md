# SCPI and VXI-11

How to control OOS from a computer: the ways in (raw socket, VXI-11/VISA, MCP), the rules of the command syntax, every command family with examples, waveform and screenshot transfer, errors, and what differs from the stock Rigol firmware. This page describes OOS 0.1.14.

> **No authentication.** Anyone who can reach the scope can control it. Keep it on a trusted bench network, never on the internet. `net.allow` (below) limits who may connect.

The complete reference with every detail is [docs/scpi.md](https://github.com/dekajoule/oos/blob/main/docs/scpi.md); the code is [ui/src/scpi.rs](https://github.com/dekajoule/oos/blob/main/ui/src/scpi.rs) with one file per subsystem in [ui/src/scpi/](https://github.com/dekajoule/oos/tree/main/ui/src/scpi). This page is the guide; where they differ, the code holds.

## Contents

- [Ways in](#ways-in)
- [First connection](#first-connection)
- [Syntax](#syntax)
- [Common commands and acquisition](#common-commands-and-acquisition)
- [Channels, timebase and trigger](#channels-timebase-and-trigger)
- [Waveform transfer](#waveform-transfer)
- [Screenshots](#screenshots)
- [Measurements](#measurements)
- [Analysers](#analysers)
- [Cursors, DVM and counter](#cursors-dvm-and-counter)
- [Math, decoders and protocol triggers](#math-decoders-and-protocol-triggers)
- [References and SIM](#references-and-sim)
- [Display, mask, histogram](#display-mask-histogram)
- [Generator and logic analyser](#generator-and-logic-analyser)
- [System and OOS extensions](#system-and-oos-extensions)
- [Synchronisation: *OPC, *ESR?](#synchronisation-opc-esr)
- [Errors](#errors)
- [Porting a Rigol script](#porting-a-rigol-script)
- [Troubleshooting](#troubleshooting)

## Ways in

All of them end in the same SCPI interpreter, so a command behaves the same whichever way it comes. The screen follows every remote change.

| Interface | Port | VISA resource | Use it for |
|---|---|---|---|
| **Raw socket** | TCP 5555 | `TCPIP::<scope>::5555::SOCKET` | scripts; the most direct way, recommended |
| **VXI-11** | 111 (portmapper, TCP and UDP), core channel 9009 | `TCPIP::<scope>::INSTR` | VISA programs that expect an LXI instrument (NI-VISA, Keysight IO, pyvisa) |
| **MCP** | 8080, `POST /mcp` | (none) | AI agents: JSON-RPC 2.0, tools `scpi` and `screenshot` |
| **HiSLIP** | 4880 | `TCPIP::<host>::hislip0::INSTR` | only the desktop build on a computer, not on the scope |

`<scope>` is the scope's address or its name, e.g. `dho924s-123456.local` ([[Remote Control]] explains the name). Computers that browse for instruments find SCPI as `_scpi-raw._tcp` and VXI-11 as `_vxi-11._tcp`.

**Switching them off:** System > Network > Remote services has a switch for SCPI and one for VXI-11 (and for the web remote, which carries MCP). A switched-off service closes its port at once.

**Who may connect:** the key `net.allow` in `/oos/system.conf` (address prefixes such as `192.168.1.0/24`) lets SCPI and VXI-11 accept only those computers; others are disconnected. Loopback always works. Details: [docs/network.md, section 6](https://github.com/dekajoule/oos/blob/main/docs/network.md#6-firewall-remote-access-and-the-mac-address).

**Limits:** 8 raw-socket clients and 8 VXI-11 connections (16 links) at once; a line can be up to 4 MiB.

**Speed:** a script that keeps a connection open costs the scope no frames (from 0.1.6).

## First connection

**Shell:**

```sh
printf '*IDN?\n' | nc dho924s-123456.local 5555
# OOS,DHO924S,<serial>,oos-ui 0.1.14
```

**Python with pyvisa, raw socket:**

```python
import pyvisa
rm = pyvisa.ResourceManager('@py')          # pyvisa-py; or the NI/Keysight backend
scope = rm.open_resource('TCPIP::dho924s-123456.local::5555::SOCKET')
scope.read_termination = '\n'
scope.write_termination = '\n'
scope.timeout = 10000                        # ms
print(scope.query('*IDN?'))
```

**Python with pyvisa, VXI-11:**

```python
scope = rm.open_resource('TCPIP::dho924s-123456.local::INSTR')
print(scope.query('*IDN?'))
```

**Python without libraries:** the example under [Waveform transfer](#waveform-transfer) uses only `socket`.

**MCP:** an agent posts JSON-RPC to `http://<scope>:8080/mcp`:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"scpi","arguments":{"command":":MEAS:ITEM? VPP,CHAN1"}}}
```

`/mcp` and `POST /update` obey `net.allow` too. The web remote's `/cmd?c=...` is not SCPI: it only takes front-panel actions (`key runstop`, `knob vscale +1`, `tap 100 200`, `drag x0 y0 x1 y1`, `workflow live|sim|load|measure|settings`).

## Syntax

- **Keywords** are case-insensitive. Use the long form or the short form (the capital letters): `:TRIGger:EDGE:SOURce?` = `:TRIG:EDGE:SOUR?`. Abbreviations between the two are not taken.
- **Lines** end with `\n` (`\r\n` works too). Answers end with `\n`.
- **Several commands** on one line are separated by `;`, each with its full header starting with `:` (no relative headers). A `;` inside quotes is not supported. Prefer one query per line when its answer itself has `;`-separated rows.
- **Numbers** are in base units, decimal or scientific: volts, seconds, hertz. Write `2e-1`, not `200mV`. Memory depth also takes `K`/`M` (`1M`).
- **Booleans** are `ON|OFF|1|0`.
- **Sources** are `CHAN1` to `CHAN4` (as the model has), `R1` to `R10` for references (`REF1` to `REF10` work too), `D0` to `D15` for the logic analyser where it applies. Use `CHAN1`, not the screen label `CH1`.
- **Queries** end in `?`. In the tables, `[?]` means set and query, `?` query only.
- **Not a number:** a value that is not available, or still being computed, reads `9.9E37`. Never treat it as a measurement.

## Common commands and acquisition

| Command | What it does |
|---|---|
| `*IDN?` | `OOS,<model>,<serial>,oos-ui <version>` |
| `*RST` | the default settings (as the Default key) |
| `*CLS` | clears the error queue, the event register and pending `*OPC` registrations |
| `*OPC?` / `*OPC` / `*ESR?` | see [Synchronisation](#synchronisation-opc-esr) |
| `:RUN`, `:STOP`, `:SINGle` | Run (on the live acquisition, after a Single with the sweep before it, Auto or Normal; a stored replay and SIM go to Normal), Stop, one acquisition then Stop |
| `:TFORce` or `:TRIGger:FORCe` | force one trigger |
| `:AUToscale` / `:AUToscale?` | start Auto / `1` while it runs |
| `:CLEar` | clear persistence and measurement statistics |
| `:ACQuire:SRATe?` | the sample rate of the ADC (a reference has its own: read its preamble) |
| `:ACQuire:MDEPth[?]` | memory depth: `AUTO`, `1000`, `1k` ... `125M` as the menu offers |
| `:ACQuire:TYPE[?]` | `NORMal\|PEAK\|AVERages\|HRESolution` |
| `:ACQuire:AVERages[?]` | averaging count, rounded to the powers of two the menu offers |
| `:ACQuire:AVERages:HARDware[?]` | OOS: averaging in the FPGA, `ON\|OFF` |
| `:ACQuire:ULTRa:MAXFrame[?]` | segment count of UltraAcquire (not the stock capture rate) |
| `:ACQuire:SEGMents[?] <n>\|OFF` | OOS: segmented recording; `:SEGMents:HARDware[?]`, `:SEGMents:INTerval[?] <s>`, `:SEGMents:TIMes?` (trigger times since the first segment, s) |
| `:ACQuire:CORes[?] MATCh\|RAW` | OOS: ADC cores matched in software, or the raw words (Acquire > ADC cores) |

## Channels, timebase and trigger

**Channels** (`<n>` = 1 to 4):

| Command | Values |
|---|---|
| `:CHANnel<n>:DISPlay[?]` | on/off |
| `:CHANnel<n>:SCALe[?]` | V/div; the nearest scale the scope has; the probe factor applies |
| `:CHANnel<n>:OFFSet[?]` | volts |
| `:CHANnel<n>:POSition[?]` | divisions |
| `:CHANnel<n>:COUPling[?]` | `DC\|AC\|GND` (`DC50` only where the board has 50 Ω) |
| `:CHANnel<n>:BWLimit[?]` | `OFF\|20M\|AUTO` or another filter of the menu |
| `:CHANnel<n>:PROBe[?]` | attenuation factor |
| `:CHANnel<n>:INVert[?]`, `:VERNier[?]` | on/off |
| `:CHANnel<n>:UNITs[?]` | the channel's unit |
| `:CHANnel<n>:TCALibrate[?]` | deskew, seconds |
| `:CHANnel<n>:BIAS[?]` | software zero correction (not a calibration) |
| `:CHANnel<n>:EXPand[?]` | vertical expansion reference |
| `:CHANnel<n>:LABel:SHOW[?]`, `:LABel:CONTent[?]` | label on/off and text |

**Timebase:**

| Command | Values |
|---|---|
| `:TIMebase[:MAIN]:SCALe[?]` | s/div, nearest step |
| `:TIMebase[:MAIN]:OFFSet[?]` | delay in seconds (`POSition` is the same) |
| `:TIMebase:DELay:ENABle[?]`, `:DELay:SCALe[?]`, `:DELay:OFFSet[?]` | zoom window on/off, s/div, centre time |
| `:TIMebase:VERNier[?]` | fine steps on/off |
| `:TIMebase:HREFerence:MODE[?]` | horizontal expansion reference |
| `:TIMebase:MODE[?]`, `:ROLL[?]` | main/zoom/XY, roll |
| `:TIMebase:XY:ENABle[?]`, `:X[?]`, `:Y[?]`, `:Z[?]` | XY display and its channels (`NONE` for no Z) |

**Trigger:**

| Command | Values |
|---|---|
| `:TRIGger:MODE[?]` (= `:TYPe[?]`) | `EDGE`, `PULSE`, `SLOPE`, `VIDEO`, `PATTERN`, `DURATION`, `TIMEOUT`, `RUNT`, `WINDOW`, `DELAY`, `SETUPHOLD`, `NTHEDGE`, `BUS`. MODE answers the short token (`EDGE`, `PULS`, `SLOP`, ...), TYPE the menu's name |
| `:TRIGger:SWEep[?]` | `AUTO\|NORMal\|SINGle` |
| `:TRIGger:STATus?` | `TD` (triggered), `WAIT`, `AUTO`, `STOP` |
| `:TRIGger:COUPling[?]` | `DC\|AC\|LFR\|HFR` |
| `:TRIGger:HOLDoff[?]` | 0 to 10 s |
| `:TRIGger:NREJect[?]` | noise reject on/off |
| `:TRIGger:EDGE:SOURce[?]` | `CHANn`, `Rn`, `D0`..`D15` |
| `:TRIGger:EDGE:LEVel[?]` | volts |
| `:TRIGger:EDGE:SLOPe[?]` | `POSitive\|NEGative\|RFALl` |
| `:TRIGger:RUNT:UPPer[?]`, `:LOWer[?]` | runt thresholds (`ALEVel`/`BLEVel` too) |
| `:TRIGger:SOURce2[?]` | second source of the types that have one |

Settings of the other trigger types that have no command here are reached through `:SYSTem:SET` (see [System](#system-and-oos-extensions)). A header that does not exist gives an error, never a silent default.

Example, a single acquisition on a rising edge at 1.5 V:

```text
:TRIG:MODE EDGE
:TRIG:EDGE:SOUR CHAN1
:TRIG:EDGE:LEV 1.5
:TRIG:EDGE:SLOP POS
:TRIG:SWE NORM
:SING
:TRIG:STAT?
```

## Waveform transfer

| Command | What it does |
|---|---|
| `:WAVeform:SOURce[?]` | `CHANn` or `Rn` to read (does not change the trigger) |
| `:WAVeform:MODE[?]` | `NORMal` (the screen record), `MAXimum`, `RAW` (the whole memory) |
| `:WAVeform:FORMat[?]` | `BYTE`, `WORD`, `ASCii` |
| `:WAVeform:STARt[?]`, `:STOP[?]` | the page to read, first point = 1, both inclusive |
| `:WAVeform:POINts[?]` | points in the page; setting it sets STOP from STARt |
| `:WAVeform:PREamble?` | 10 values, below |
| `:WAVeform:DATA?` | the samples as an IEEE block |

**Modes.** NORM reads the record on screen. RAW reads the samples in memory, also outside the screen; OOS allows RAW while running (the stock firmware only when stopped). MAX is the screen record while running and the memory when stopped. A reference gives its own samples in every mode. While running, two queries can see two different acquisitions: send `:STOP` first when preamble and data must belong together.

**Preamble:** `format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference`. `format` 0 BYTE, 1 WORD, 2 ASCII; `type` 0 NORM, 1 MAX, 2 RAW; `count` is 1.

```text
time[k]  = (k - xreference) * xincrement + xorigin
volts[k] = (code[k] - yorigin - yreference) * yincrement
```

**Formats.**
- BYTE: unsigned 8 bit.
- WORD: unsigned 16 bit, **little-endian**. Stock Rigol scripts often assume another order: check yours.
- ASCII: comma-separated volts (7 digits, scientific), already scaled. Do not apply the formula again.

**IEEE block:** `#` then one digit N, then N digits of length, then the payload, then `\n`. Read exactly that many bytes; never split binary data at newlines. A page larger than 64 MiB encoded is refused with error -223: read in pages with `:WAV:STARt`/`:WAV:STOP` (the preamble's `xorigin` is then the time of the page's first point). WORD needs far less memory than ASCII.

**Python with pyvisa:**

```python
scope.write(':STOP')
scope.write(':WAV:SOUR CHAN1')
scope.write(':WAV:MODE RAW')
scope.write(':WAV:FORM WORD')
pre = [float(x) for x in scope.query(':WAV:PRE?').split(',')]
codes = scope.query_binary_values(':WAV:DATA?', datatype='H', is_big_endian=False, container=list)
volts = [(c - pre[8] - pre[9]) * pre[7] for c in codes]
times = [(k - pre[6]) * pre[4] + pre[5] for k in range(len(codes))]
print(len(volts), 'points, dt =', pre[4])
```

**Python with the standard library only:**

```python
import socket, struct

with socket.create_connection(('dho924s-123456.local', 5555), timeout=10) as s:
    s.settimeout(120)

    def send(cmd):
        s.sendall((cmd + '\n').encode())

    def exact(n):
        b = bytearray()
        while len(b) < n:
            part = s.recv(min(65536, n - len(b)))
            if not part:
                raise EOFError('scope disconnected')
            b += part
        return bytes(b)

    def query(cmd):
        send(cmd)
        line = bytearray()
        while (c := exact(1)) != b'\n':
            line += c
        return line.decode().strip()

    send(':STOP'); send(':WAV:SOUR CHAN1'); send(':WAV:MODE RAW'); send(':WAV:FORM WORD')
    pre = [float(x) for x in query(':WAV:PRE?').split(',')]
    send(':WAV:DATA?')
    assert exact(1) == b'#'
    payload = exact(int(exact(int(exact(1)))))
    exact(1)                                   # the closing \n
    codes = struct.unpack(f'<{len(payload) // 2}H', payload)
    print(len(codes), 'points, first:', (codes[0] - pre[8] - pre[9]) * pre[7], 'V')
    print(query(':SYST:ERR?'))
```

## Screenshots

`:DISPlay:DATA? [BMP|PNG]` returns the screen as an IEEE block, BMP (24 bit, 1.8 MB) by default, PNG on request (60 to 175 KB).

```python
png = scope.query_binary_values(':DISP:DATA? PNG', datatype='B', container=bytes)
open('screen.png', 'wb').write(png)
```

Without SCPI: `http://<scope>:8080/screen.png` (or `.bmp`) gives the same picture.

## Measurements

| Command | What it does |
|---|---|
| `:MEASure:ITEM? <item>[,<source>]` | one value; without a source the one selected in Measure |
| `:MEASure:ITEM? <pair>,<A>,<B>` | a value between two sources (delay, phase, skew, setup/hold) |
| `:MEASure:ITEM <item>[,<A>[,<B>]]` | adds the measurement to the strip on screen |
| `:MEASure:SOURce[?] <A>[,<B>]` | default sources |
| `:MEASure:CLEar` | removes all measurements and resets statistics |
| `:MEASure:GATing[?]` | `SCREEN` (what is shown) or `NONE` (the whole record). OOS |
| `:MEASure:REFS[?]` | `10/90` or `20/80` edge thresholds |
| `:MEASure:SETup:MIN[?]`, `:MAX[?]` | low/high thresholds in percent |
| `:MEASure:SETup:PSA[?]`, `:PSB[?]`, `:DSA[?]`, `:DSB[?]` | phase and delay sources A/B |
| `:MEASure:TSETup[?]`, `:THOLd[?]` | setup and hold time limits, s |
| `:MEASure:BURSt:IDLE[?]`, `:OUTSide[?] ABOVe\|BELow\|BOTH`, `:NPERiods[?]`, `:EDGes[?] CLOCk\|DATA` | settings of the measurements that use them |
| `:MEASure:INDicator[?]` | markers on the trace (`:CURSor:MEASure:INDicator` too) |
| `:MEASure:STRip?` | every row of the strip, `;`-separated |
| `:MEASure:READout?` | the strip plus the eye figures while the eye page is open |
| `:MEASure:STATistic:DISPlay[?]` | statistics on/off (keeps what is counted) |
| `:MEASure:STATistic:ITEM <item>[,src[,src]]` | adds a row with statistics |
| `:MEASure:STATistic:ITEM? <field>,<item>[,src[,src]]` | `MAXimum\|MINimum\|CURRent\|AVERages\|DEViation\|CNT` |
| `:MEASure:STATistic:RESet` | resets statistics |

**Rows** of `STRip?`/`READout?`: `ITEM,SOURCE,value`; pairs `ITEM,A,B,value`; catalog rows `CAT<id>,A,B,C,D,value`; DVM `DVM,mode,SOURCE,value`; counter `COUNTER,mode,SOURCE,value`; eye `FIGURE,value`.

**Single values:** `VPP`, `VMAX`, `VMIN`, `VAVG`, `VRMS`, `VAMP`, `FREQUENCY`, `PERIOD`, `RTIME`, `FTIME`, `PWIDTH`, `NWIDTH`, `PDUTY`, `NDUTY`, `VTOP`, `VBASE`, `OVERSHOOT`, `PRESHOOT`, `HTIME`, `LTIME`, `BWIDTH`, `TOUTSIDE`, `NPERIODS`, `CAMP`, `CTOP`, `CBASE`, `CPP`, `CMAX`, `CMIN`, `VUPPER`, `VMID`, `VLOWER`, `CAVG`, `CRMS`, `ACRMS`, `SDEVIATION`, `AREA`, `CAREA`, `PPULSES`, `NPULSES`, `PEDGES`, `NEDGES`, `TVMAX`, `TVMIN`, `PSLEW`, `NSLEW`, `ESTUI`, `MEDIAN`, `MODE`, `LSLO`, `LINT`, `PAM0`..`PAM3`, `PSEP`, `BITS`, `PLLUI`, `UIWTD`, `FOLD`.

**Pairs:** `RRDELAY`, `FFDELAY`, `RFDELAY`, `FRDELAY`, `RRPHASE`, `FFPHASE`, `RFPHASE`, `FRPHASE`, `RRSKEW`, `FFSKEW`, `RFSKEW`, `FRSKEW`, `SETUP`, `HOLD`, `SETHOLD`, `FSETUP`, `FHOLD`.

Values are in base units, phase in degrees, duty and overshoot in percent. `*OPC?` does not wait for measurements: poll until the value is not `9.9E37`.

```text
:MEAS:GAT NONE
:MEAS:ITEM? VPP,CHAN1
:MEAS:ITEM? FREQUENCY,CHAN1
:MEAS:ITEM? RRDELAY,CHAN1,CHAN2
:MEAS:STAT:ITEM? DEV,PERIOD,CHAN1
```

## Analysers

| Command | What it does |
|---|---|
| `:VIEW[?]` | the page: `SCOPE\|SPECTRUM\|EYE\|POWER\|BODE\|MAGNETIC\|INVERTER\|DPT` |
| `:EYE:ITEM? <figure>` | eye figures: `ONE`, `ZERO`, `AMPLITUDE`, `CROSSING`, `RISE`, `FALL`, `HEIGHT`, `WIDTH`, `Q`, `HBER`, `UI`, `TJ`, `RJ`, `DJ`, `DCD`, `DDJ`, `TPP`, `TRMS`, `J2`, `J9`, `OPENING`, ... (all in [docs/scpi.md](https://github.com/dekajoule/oos/blob/main/docs/scpi.md#eye-figures)) |
| `:SPECtrum:ITEM? <item>[,src]` | `F1`, `THD`, `THDPCT`, `THDN`, `SNR`, `SINAD`, `SFDR`, `ENOB`, `ACPR`, `ACPRLO`, `ACPRHI`, `FLOOR` |
| `:POWer:ITEM? <item>[,V,I]` | `VRMS`, `IRMS`, `REAL`, `APP`, `REAC`, `PF`, `PHAS`, `FREQ`, `EON`, `EOFF`, `ECOND`, `RDSON`, ripple items |
| `:POWer:HARMonics? [src]` | harmonic number and amplitude pairs, `;`-separated |
| `:MAGnetic:ITEM? <item>[,V,I]` | `L`, `B`, `H`, `MUR`, `LOSS` (needs the core geometry) |
| `:INVerter:ITEM? <item>[,src...]` | the inverter page's results |
| `:DPT:ITEM? <item>[,V,I[,G]]` | double pulse: `VPEAK`, `IPEAK`, `EON`, `EOFF`, `TDON`, `TR`, `TDOFF`, `TF`, `QRR` |
| `:BANDwidth:ITEM? <item>[,src]` | `RISE`, `BW`, `OVER` |
| `:BODE:MODE[?] LOOP\|PSRR\|IMPedance`, `:BODE:RUN ON\|OFF`, `:BODE:DATA?`, `:BODE:GENerator[?] "ip:port"` | Bode plot; RUN drives the generator |
| `:IDENtify?` | what Identify recognises in the signal |

These are OOS functions, not stock Rigol commands. Their limits are in [reference analysis](https://github.com/dekajoule/oos/blob/main/docs/reference-analysis.md).

## Cursors, DVM and counter

| Command | Values |
|---|---|
| `:CURSor:MODE[?]` | `OFF\|MANual\|TRACk\|XY` |
| `:CURSor:MANual:TYPE[?]` | `TIME\|AMPLitude` |
| `:CURSor:{MANual\|TRACk}:SOURce[?]` | `CHANn\|Rn` |
| `:CURSor:TRACk:SOURce1[?]`, `:SOURce2[?]` | Track: cursor A and B each on their own source (B follows A when unset) |
| `:CURSor:{MANual\|TRACk}:CAX[?]`, `:CBX[?]` | A/B time in seconds |
| `:CURSor:{MANual\|TRACk}:CAY[?]`, `:CBY[?]` | A/B volts |
| `:CURSor:XY:{AX\|BX\|AY\|BY}[?]` | XY cursor positions |
| `:CURSor:{MANual\|TRACk\|XY}:{AXValue\|BXValue\|AYValue\|BYValue\|XDELta\|IXDelta\|YDELta}?` | readouts; `IXDelta` = 1/Δt |
| `:DVM:ENABle[?]`, `:SOURce[?]`, `:MODE[?] DC\|ACRMS\|ACDC`, `:CURRent?` | voltmeter |
| `:COUNter:ENABle[?]`, `:SOURce[?]`, `:MODE[?] FREQuency\|PERiod\|TOTalize`, `:NDIGits[?] 3..7`, `:CURRent?`, `:TOTalize:CLEar` | frequency counter |

## Math, decoders and protocol triggers

**Math** (`<n>` = slot): `:MATH<n>:DISPlay[?]`, `:OPERator[?]`, `:SOURce1[?]`, `:SOURce2[?]`, `:SCALe[?]`, `:OFFSet[?]`, `:EXPRession[?]` (a formula, e.g. `CHAN1 - CHAN2` or `R1 - R2`), `:FFT:SOURce[?]`, `:FFT:WINDow[?]`, `:FFT:SCALe[?]`, `:FILTer:TYPE[?]`, `:FILTer:W1[?]`, `:FILTer:W2[?]`. A Math source can be a channel, a reference or an earlier Math slot.

**Decoders, stock names** (`<n>` = slot): `:BUS<n>:MODE[?]` (protocol), `:DISPlay[?]`, `:FORMat[?] HEX|ASCii`, `:EVENt[?]`, `:THReshold[?]`, `:DATA?` (events as an IEEE block, `BUSY` while decoding), `:BUS<n>:RS232:TX[?]`, `:RX[?]`, `:BAUD[?]`, `:PARity[?]`, `:DBITs[?]`, `:BUS<n>:IIC:SCLK:SOURce[?]`, `:SDA:SOURce[?]`, `:BUS<n>:SPI:SCLK:SOURce[?]`, `:MOSI:SOURce[?]`, `:SS:SOURce[?]`, `:DBITs[?]`.

**Decoders, every protocol** (works for all of them, e.g. CAN, LIN, USB, C-PHY):

| Command | Values |
|---|---|
| `:DECode:SELect[?]` | decoder slot, from 1 |
| `:DECode:PROTocol[?] <name>` | the menu's name, case and punctuation ignored (`I2C`, `SPI`, `CAN FD`, `C-PHY`, ...) |
| `:DECode:STATe[?]` | on/off |
| `:DECode:INPut[?]` | `WAVEFORM` (or `ANALOG`) or `LA` |
| `:DECode:SOURce[?]`, `:SOURce2[?]`, `:CS[?]`, `:SOURce4[?]`, `:DIFFerential[?]` | the protocol's lines, `CHANn\|Rn\|NONE` |
| `:DECode:THReshold[?]` | threshold |
| `:DECode:CPHY:RATE[?]` | `AUTO` or symbols/s |
| `:DECode:BUSY?`, `:DECode:EVENts?` | still decoding? / the events with their times |

**Protocol triggers** (stock names, plus OOS additions; values decimal or `0x` hex):

| Command | Values |
|---|---|
| `:TRIGger:RS232:WHEN` | `STARt\|DATA\|ERRor\|CERRor\|EITHer` |
| `:TRIGger:RS232:POLarity`, `:STOP`, `:PARity`, `:WIDTh`, `:BAUD`, `:DATA` | `POSitive\|NEGative`, `1\|1.5\|2`, `NONE\|EVEN\|ODD`, 5..8, baud, value |
| `:TRIGger:SPI:WHEN` | `CS\|TIMeout`; `:TIMeout <s>`, `:SLOPe`, `:MODE HIGH\|LOW`, `:WIDTh 4..32`, `:DATA` |
| `:TRIGger:IIC:WHEN` | `STARt\|ADDRess\|DATA\|NACKnowledge`; `:AWIDth 7\|10`, `:ADDRess`, `:DIRection READ\|WRITe\|RWRite`, `:DBYTes 0..5`, `:DATA` |
| `:TRIGger:<bus>:RELation`, `:DMAX`, `:MASK` | OOS: `EQ\|NE\|GT\|LT\|IN\|OUT`, upper limit, mask |
| `:TRIGger:IIC:ARELation`, `:AMAX` | OOS: relation and upper limit of the address |

## References and SIM

| Command | What it does |
|---|---|
| `:REFerence:SLOT[?] R1..R10`, `:CURRent[?] 1..10` | the working slot |
| `:REFerence:LOAD "path"` | load a file from the scope's storage into the slot (a path on the scope, not an upload) |
| `:REFerence:IMPort ["path"]` | load a file, or the slot's file next to the setups |
| `:REFerence:SAVE` | copy the source channel's record into the slot |
| `:REFerence:SOURce[?] [<slot>,]CHANn` | which channel SAVE takes |
| `:REFerence:SHOW[?] [<slot>,]ON\|OFF` | show or hide (`DISPlay` too) |
| `:REFerence:VSCale[?]`, `:VOFFset[?]`, `:COLor[?]`, `:LABel:CONTent[?]`, `:RESet` | how it is shown (the samples stay) |
| `:REFerence:CLEar` | empty all slots |
| `:SIM:PRESet[?]`, `:SLOT[?]`, `:PATTern[?] PRBS7..PRBS31`, `:PLAY[?] ON\|OFF`, `:BUSY?`, `:SHOW[?]` | the signal simulator; settings take effect on the next `:SIM:PLAY ON` |
| `:SIM:SSC:MODE[?] OFF\|DOWN\|CENTER`, `:SSC:DEViation[?]`, `:SSC:FREQuency[?]` | spread spectrum |
| `:SIM:JITTer:DCD[?]`, `:CORRelated[?]`, `:CORNer[?]` | jitter |

To upload a file from the computer, use the web remote (`POST /waveform?name=...&slot=1`), not `:REF:LOAD`.

A reference can be a trigger, measurement, cursor, Math or decoder source like a channel; triggering on it searches the stored samples (OOS).

## Display, mask, histogram

| Command | Values |
|---|---|
| `:DISPlay:TYPE[?]` | `DOTS\|VECTors` |
| `:DISPlay:GRADing:TIME[?]` | `MIN`, `0.1`, `0.5`, `1`, `5`, `10`, `INFinite` |
| `:DISPlay:WBRightness[?]`, `:GBRightness[?]`, `:GRID[?]`, `:RULers[?]`, `:COLor[?]` | waveform/grid brightness, grid, rulers, colour grading |
| `:DISPlay:VIEW[?] <n>`, `:VIEW:COUNt[?] 1..4`, `:VIEW:MONitor[?] 0..4` | OOS: the views of Display > Views, and the one an HDMI monitor shows |
| `:MASK:ENABle[?]`, `:SOURce[?]`, `:X[?]`, `:Y[?]`, `:CREate`, `:RESet`, `:OPERate[?]`, `:FAILed?`, `:PASSed?`, `:TOTal?` | mask test |
| `:HISTogram:ENABle[?]`, `:TYPE[?]`, `:SOURce[?]`, `:STATistics:RESult?` | histogram of what is shown |

## Generator and logic analyser

**Generator** (S models): `:SOURce[1]:OUTPut[:STATe]`, `:FUNCtion SINusoid|SQUare|RAMP|PULSe|NOISe|DC|ARB`, `:FREQuency`, `:VOLTage[:AMPLitude]` (Vpp), `:VOLTage:OFFSet`, `:FUNCtion:SQUare:DUTY`, `:FUNCtion:RAMP:SYMMetry`, `:ARB:DATA`, `:ARB:CAPTure`, `:MOD[:STATe]`, `:MOD:TYPe AM|FM|PM`, `:MOD:AM:DEPTh`, `:MOD:FM:DEViation`, `:MOD:PM:DEViation`, `:MOD:<type>:INTernal:FREQuency|FUNCtion`, all with queries. On a model without the generator they report hardware missing. See [[Generator]].

**Logic analyser** (with the PLA2216 probe): `:LA:STATe[?]`, `:LA:DIGital<n>:DISPlay[?]`, `:LA:POD<n>:THReshold[?]`, `:LA:POINts?`, `:LA:PREamble?`, `:LA:DATA?` (an IEEE block of 16-bit little-endian words, bit n = Dn). See [[Logic Analyser]].

## System and OOS extensions

| Command | What it does |
|---|---|
| `:SYSTem:ERRor?` | the next error, see [Errors](#errors) |
| `:SYSTem:STATe?` | diagnostic state of the application (also whether the GPU draws) |
| `:SYSTem:GET? <key>`, `:SYSTem:SET <key> = <value>` | any setting by its setup key, also those without a command of their own (keys in [lib.rs of oos-store](https://github.com/dekajoule/oos/blob/main/ui/store/src/lib.rs)) |
| `:SYSTem:GRAPhics[?] CPU\|GPU` | System > Graphics |
| `:SYSTem:AOUTput[?] TOUT\|PFAil\|OFF` | the rear AUX OUT: trigger out, pass/fail (where the mask or zone unit is there), or low |
| `:SYSTem:LOG ON\|OFF` | measurement logger |
| `:CALibrate:STARt 0..4[,ZERO]`, `:STATus?`, `:CANCel` | starts self-calibration: **inputs disconnected first**. Not a passive read |

`:SYSTem:DEBug:*` commands exist for development; they read raw hardware blocks and are not a stable interface.

## Synchronisation: *OPC, *ESR?

- `*OPC?` waits until the operation in progress is finished, then answers `1`. It waits for Auto, a SIM generation, an armed `:SINGle`, and a `:STOP` until the record of the stop is in place (a moment, at most 3 s): after `:STOP;*OPC?` the preamble and the data belong to the same record. It does **not** wait for continuous Run, measurements, Math, decoding or the eye: poll their values instead.
- `*OPC` sets bit 0 of the event register when they are finished; `*ESR?` reads and clears it. Bits: 0 operation complete, 2 query error, 3 device error, 4 execution error, 5 command error.
- A `:SINGle` that never triggers makes `*OPC?` wait forever: set a timeout in the client, or `:TFORce`.
- While a client waits, the scope keeps running and serves other clients.
- Use `*OPC?` over the raw socket. Over VXI-11 a read can end at once with timeout error 15 while the answer is still pending: the client has to read again. Through `/mcp` a waiting `*OPC?` is refused (-221).

```text
:SING
*OPC?          # answers 1 once the acquisition is done
:MEAS:ITEM? VPP,CHAN1
```

## Errors

Read `:SYSTem:ERRor?` until it answers `0,"No error"`. Each read takes one error off the queue (at most 20; more gives -350). A query that fails answers with the error text right away, so a client never hangs on it.

| Code | Meaning |
|---|---|
| -113 | undefined header (the command does not exist) |
| -221 | settings conflict (e.g. `*OPC?` through MCP) |
| -223 | too much data (a waveform page above 64 MiB) |
| -224 | illegal parameter value |
| -230 | data not available (analysis without a result) |
| -241 | hardware missing (e.g. generator on a model without it) |
| -256 | file not found |
| -350 | queue overflow |
| -410 | query interrupted (the operation was cancelled) |

`:RUN?`, `:STOP?`, `:SING?`, `*RST?` and `*CLS?` are not queries; they give an error and do not run the action.

## Porting a Rigol script

OOS takes the stock DHO800/DHO900 command names where it has the function, but it is not the stock firmware. Check these points:

- **`*IDN?`** starts with `OOS`, not `RIGOL TECHNOLOGIES`. A script that checks the maker has to accept it.
- **WORD data** is little-endian.
- **RAW while running** is allowed; stock scripts that stop first still work.
- **Setup files** of the stock firmware are not read; OOS has its own setups.
- **Status system:** no SRQ and no full status subsystem, only `*ESR?` and `*OPC`.
- **Trigger subtrees:** not every type-specific stock command exists; use `:SYSTem:SET` or check the error.
- **Units in numbers:** `200mV` is not taken, write `0.2`.
- **Tektronix** commands (`CURVe?`, `WFMOutpre`, ...) are not understood.

After configuring, read back the settings and check `:SYST:ERR?`: an OOS value can be rounded to the nearest step the scope has.

## Troubleshooting

| Symptom | What to check |
|---|---|
| connection refused | System > Network > Remote services: SCPI or VXI-11 on? `net.allow` set and your computer not in it? |
| VISA does not find the scope | VXI-11 uses ports 111 and 9009: a firewall between must let both through. Try the raw socket `TCPIP::<scope>::5555::SOCKET` |
| `.local` name not found | mDNS on (Remote services)? Same network? Use the IP address from System > Network |
| query hangs | the line must end in `\n`; for pyvisa set `read_termination = '\n'` on SOCKET resources |
| binary data broken | read the IEEE block by its length, not up to a newline; WORD is little-endian |
| value `9.9E37` | not available yet or not measurable: wait and ask again, check source and gating |
| `*OPC?` never answers | a `:SINGle` without a trigger: `:TFORce`, or a timeout |

Logs: `/tmp/oos-ui.log` on the scope (over SSH). Report problems with the command, the answer and `:SYST:ERR?` in an [issue](https://github.com/dekajoule/oos/issues).
