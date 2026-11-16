# OOS Plus SCPI reference

OOS Plus implements a **subset of Rigol DHO800/DHO900 SCPI**, including commands used by DHO924S scripts, plus OOS analysis extensions. A script works when its commands, parameters and transfer assumptions are in this reference. OOS does not implement the complete stock firmware command set, impersonate Rigol, or speak TekScope's command dialect. `*IDN?` identifies OOS.

This reference describes the repository implementation. Primary Rigol syntax was checked against the [DHO800/DHO900 Programming Guide](https://download.rigol.com/en/Manual/Digital%20Oscilloscope/DHO800/DHO800900_ProgrammingGuide_EN.pdf), including trigger MODE and waveform preamble definitions. Local backend tests establish the implemented behavior; they do not establish physical instrument or VISA-driver certification. See [local validation](../local-validation.md). The canonical implementation is [scpi.rs](../../ui/src/scpi.rs) with the tables in [scpiext*.rs](../../ui/src/ui/scpiext.rs); transport details are in [hislip.rs](../../ui/src/hislip.rs) and [vxi11.rs](../../ui/src/vxi11.rs).

Every command has exactly one row, in exactly one of the group pages below (568 headers in the tables). Menu equivalents use the UI labels, as Area > Option.

## Contents

- [Connection](#connection)
- [Syntax and conventions](#syntax-and-conventions)
- [How to read the tables](#how-to-read-the-tables)
- [Quick start](#quick-start)
- [Command groups](#command-groups)
- [Operation completion](#operation-completion)
- [Error codes](#error-codes)
- [Compatibility limits](#compatibility-limits)

## Connection

| Interface | Default | Implementation |
| --- | --- | --- |
| Raw TCP SCPI | TCP 5555 | Recommended for scripts. Newline-terminated program messages; newline-terminated replies. |
| HiSLIP | TCP 4880 | Same interpreter, with HiSLIP framing. Basic sessions; full locking/overlap semantics are not implemented. |
| VXI-11 | Linux scope service | Same interpreter, partial RPC transport. Pending operation reads have the limitation described under operation completion. Not advertised by the Windows app. |
| HTTP/MCP adapter | HTTP web port | `POST /mcp`, JSON-RPC `tools/call` with tool name `scpi`. This is not a VISA interface. `/cmd` uses a separate front-panel command language. |

The source launcher can use different ports. For example:

```powershell
.\run-windows.ps1 -Watch -Offline -WebPort 18080 -ScpiPort 5555 -HislipPort 14880
```

For that launcher, use `TCPIP0::127.0.0.1::5555::SOCKET`, or HiSLIP resource `TCPIP0::127.0.0.1::hislip0,14880::INSTR`. On another computer, substitute the PC's LAN address. These services are unauthenticated bench interfaces. The desktop app does not require or start an SSH server; [scope-image SSH](../ssh.md) is separate. The Remote services switches are in [system](system.md#remote-services).

## Syntax and conventions

- Keywords are case-insensitive. The capitalised part of a mixed-case spelling is the short form: `:TRIG:EDGE:SOUR?` and `:TRIGger:EDGE:SOURce?` are equivalent. Use either the complete long form or its documented short form; intermediate abbreviations are not generally accepted.
- Use an absolute header for every semicolon-separated command. The parser splits literal semicolons; relative-header inheritance and quoted strings containing semicolons are unsupported. Prefer one query per message when replies themselves contain semicolon-separated rows. A definite-length block `#<n><length><data>` after a blank is not split.
- `<n>` in a header is a numeric suffix (`CHANnel<n>` is `CHAN1`).
- Numeric setters accept finite decimal or scientific notation in base units: volts, seconds, hertz. Use `2e-1`, not `200mV`. Memory depth also accepts K/M suffixes. Replies are scientific notation with six decimals (`1.000000E+00`).
- Booleans are `ON|OFF|1|0`; queries answer `1` or `0`.
- A choice item takes the long or short keyword of its option text (`CHANnel2` for CH2; case, spaces and dashes of the option text do not matter) and answers the option text in capitals, spaces as `_`, or a short token.
- Waveform names are `CHAN1` to `CHAN4` as available on the model, and `R1` to `R10` (`REF1` to `REF10` are aliases); a source may also be `MATHn` where a table says so. Use `CHAN1`, not the UI label `CH1`, in scripts. Reference sources are an OOS extension; an absent reference is not a physical input.
- Unavailable or pending numeric values read `9.9E37`, not a real measurement.
- Commands that power off, restart, update, roll back, change the boot mode, overclock, go back to stock, or switch the SCPI service off take a literal `CONFIRM` argument and fail with -221 without it.
- Action queries such as `:RUN?`, `:STOP?`, `:SING?`, `*CLS?` and `*RST?` are unsupported and do not execute the action.

## How to read the tables

Each group page has an overview, the menu location, a command table and worked examples.

| Column | Meaning |
| --- | --- |
| Command | Long form with the short form capitalised (`:TRIGger:EDGE:SLOPe` short is `:TRIG:EDGE:SLOP`). `<n>` is a numeric suffix; `[<n>]` is optional; `{A\|B}` means one of the alternatives. |
| Set/Query | `Set / Query`: the header takes a value and answers `<header>?`. `Query`: query only. `Set`: set only (takes a value). `Action`: does something and has no query; some take a path or a slot as argument. |
| Parameters and range | Argument forms, values, units and limits. Limits are those of the code; the setup headers show the range checked by the interpreter. Backticked `<...>` is a placeholder, `[...]` optional. |
| Default | What `<header>?` answers on a freshly started default model with the demo source (the same state the Default setup gives). A dash: no query, an action, or a reply that needs a loaded reference, a protocol or another condition. The value depends on the model (a DHO924S has four channels). |
| Menu equivalent | The menu item that does the same thing, as Area > Option. |
| Notes | The origin of the name. **Rigol-compatible**: the header is in the checked Rigol DHO subset with the same name. **Rigol-style (command family of the DHO guide)**: the family exists in the guide (sections 3.4, 3.6, 3.8, 3.13, 3.15 to 3.17 and 3.26) but this exact header is not individually verified. **name not verified against the DHO guide**: named after the Rigol MSO style from memory, flagged "verify" by the coverage report, and not yet compared with the DHO800/900 guide. **Rigol-style spelling, not listed in the coverage report**: no verdict was recorded. **OOS extension**: a plain top-level subtree like `:SIM:` or `:BODE:`, no `:OOS:` prefix. |

## Quick start

```text
*IDN?                          -> OOS,DHO924S,0,oos-ui 0.1.12
:STOP
:CHANnel1:SCALe 0.5
:CHANnel1:SCALe?               -> 5.000000E-01
:TRIGger:EDGE:SOURce CHAN1
:TRIGger:EDGE:LEVel 1
:TRIGger:SWEep NORMal
:SINGle
:TRIGger:STATus?
:MEASure:ITEM? VPP,CHAN1
:SYSTem:ERRor?                 -> 0,"No error"
```

### Reading a waveform in Python

A minimal standard-library reader for the running Windows app. It stops acquisition and keeps that state, and reads one payload rather than constructing millions of Python float objects.

```python
import socket
import struct

with socket.create_connection(('127.0.0.1', 5555), timeout=10) as scope:
    scope.settimeout(120)

    def send(command):
        scope.sendall((command + '\n').encode('ascii'))

    def exact(n):
        data = bytearray()
        while len(data) < n:
            part = scope.recv(min(65536, n - len(data)))
            if not part:
                raise EOFError('scope disconnected')
            data.extend(part)
        return data

    def query(command):
        send(command)
        line = bytearray()
        while True:
            ch = exact(1)
            if ch == b'\n':
                return line.decode('ascii').strip()
            line.extend(ch)

    print(query('*IDN?'))
    send(':STOP')
    send(':WAV:SOUR R1')  # Load R1 first, or use CHAN1.
    send(':WAV:MODE RAW')
    send(':WAV:FORM WORD')
    pre = [float(x) for x in query(':WAV:PRE?').split(',')]
    send(':WAV:DATA?')
    assert exact(1) == b'#'
    digits = int(exact(1))
    payload = exact(int(exact(digits)))
    assert exact(1) == b'\n'
    assert len(payload) == 2 * int(pre[2])
    if payload:
        code = struct.unpack_from('<H', payload)[0]
        print('points:', int(pre[2]), 'dt:', pre[4],
              'first volts:', (code - pre[8] - pre[9]) * pre[7])
    print(query(':SYST:ERR?'))
```

See [waveform](waveform.md) for the preamble and the formats.

## Command groups

| Page | Commands | Covers |
| --- | --- | --- |
| [Common commands](common.md) | 7 | IEEE 488.2: `*IDN?`, `*RST`, `*CLS`, `*OPC`, `*ESR?`, `*SAV`, `*RCL` |
| [System](system.md) | 41 | Identification, setup block, date, time, NTP, network, remote services, power, update, boot |
| [Run control and acquisition](acquire.md) | 16 | Run, stop, single, autoset, clear; acquisition type, depth, history |
| [Channels](channel.md) | 17 | Vertical settings per channel, active channel |
| [Timebase](timebase.md) | 13 | Main, delayed, roll and XY time base |
| [Trigger](trigger.md) | 72 | Type, sweep, level and one subsection per trigger type |
| [Waveform transfer](waveform.md) | 8 | Waveform transfer, preamble, formats |
| [Measurements](measure.md) | 41 | Items, pairs, statistics, gate, levels, catalogue |
| [Cursors, DVM and counter](cursor.md) | 30 | Cursors, DVM, counter |
| [Math](math.md) | 16 | Math channels, FFT, filters, formulas |
| [Reference waveforms](reference.md) | 19 | Reference slots, files, export, look, per-reference horizontal scale |
| [Search (mark events)](search.md) | 13 | Mark events and walk through them |
| [Decoders and bus](decode.md) | 51 | Decoders, bus parameters, plot bus |
| [Logic analyser](logic.md) | 9 | Logic analyser lines and pods |
| [Mask, zones and histogram](mask.md) | 26 | Mask test, zones, histogram |
| [Spectrum](spectrum.md) | 17 | Spectrum analyser setup and figures |
| [Eye and jitter](eye.md) | 15 | Eye diagram and jitter |
| [Power, magnetic, inverter and double pulse](power.md) | 51 | Power, magnetic, inverter, double pulse |
| [Bode and bandwidth test](bode.md) | 21 | Bode sweep and bandwidth test |
| [Built-in generator](generator.md) | 18 | Built-in generator, modulation, arbitrary wave |
| [Signal simulator (SIM)](sim.md) | 26 | Signal simulator |
| [Display, views and Studio zoom](display.md) | 23 | Display, views, Studio zoom, screenshot |
| [Save and load](storage.md) | 8 | Save and load setups, waveforms, images, fast frames |
| [Data logger](logger.md) | 4 | Data logger |
| [Calibration and deskew](calibration.md) | 6 | Self-calibration and analysis deskew |
| [Setup keys](setup-keys.md) | - | The `:SYSTem:SET` key list and the packed-key formats |

Cross-links: `:TFORce` is in [acquire](acquire.md) and `:TRIGger:FORCe` in [trigger](trigger.md); `*SAV` / `*RCL` are in [common](common.md) and the save and load headers in [storage](storage.md); the Studio zoom headers `:TIMebase:DELay:SOURce` and siblings are in [display](display.md#studio-zoom); `:EYE:ITEM?` is in [eye](eye.md) and `:MEASure:ITEM` in [measure](measure.md); the logger headers `:SYSTem:LOG...` are in [logger](logger.md) although they share the `:SYSTem:` subtree.

## Operation completion

`*OPC?` fences the finite operation current at the barrier: Autoset, a submitted SIM generation, and an armed Single. Continuous Run and background measurement, Math, decode and Eye calculations are not drained by OPC. Use their readiness or value interfaces separately. Operation completion is not a statement that a result is valid; inspect the error queue and trigger status.

Raw TCP and HiSLIP retain a bounded per-client continuation while waiting, allowing UI progress and other clients. Commands after the barrier execute after it completes; earlier writes are not repeated. A Single with no matching event can wait indefinitely, so choose a client timeout appropriate to the test. Cancellation or rearming interrupts a waiting query with error -410, emits no `1`, and discards its remaining commands; a cancelled `*OPC` registration does not set the completion bit.

The synchronous command/HTTP adapter cannot retain a pending `*OPC?` and reports error -221 instead of returning a false `1` or `ok`. Commands preceding the rejected barrier have already executed; its remaining commands are discarded. VXI-11's legacy read path can return timeout error 15 immediately while output is pending, without honoring the requested read timeout, so a VXI client must retry reads after completion; prefer raw TCP or HiSLIP for a waiting `*OPC?`. Full VISA synchronization equivalence is not claimed.

Run, stop and Single on a loaded reference or SIM: stored Run searches successive qualifying windows through the immutable record and wraps at its end. It does not create new ADC samples. SIM produces fresh candidates; a candidate replaces accepted data only when its trigger qualifies, Auto permits fallback, or Force is requested. Normal and Single misses remain WAIT and retain the accepted picture. Stopped panning updates Screen analysis without adding captures or statistics observations.

## Error codes

Read `:SYSTem:ERRor?` until `0,"No error"`. Each read removes one queued error. The queue holds at most 20 bounded messages; overflow is -350. Errors latch their event bit once; draining old errors does not relatch it. Unsupported or failed queries reply with specific error text as well as queuing it, even when full. A legitimate pending `*OPC?` keeps its deferred response.

| Code | Meaning |
| --- | --- |
| -113 | Undefined header (also a sub-header the tables do not list). |
| -221 | Settings conflict: nothing to do it with, a missing `CONFIRM`, an operator that is not a filter, or a synchronous adapter that cannot hold `*OPC?`. |
| -223 | Waveform page too large (worst-case encoded size over 64 MiB). |
| -224 | Illegal parameter value (out of range, unknown option, unavailable source). |
| -230 | Data corrupt or stale: unavailable analysis, empty reference, no waveform to build a mask from. |
| -241 | Hardware missing (for example a generator on a model without one). |
| -256 | File missing. |
| -350 | Queue overflow. |
| -410 | A waiting query was cancelled or rearmed. |
| 15 | VXI-11 only: read timeout while output is pending. |

## Compatibility limits

A Rigol script commonly needs adjustments when it assumes stock `*IDN?`, big-endian WORD assumptions, vendor-specific setup files, a full status/SRQ system, arbitrary trigger-specific subtrees, or references as physical ADC inputs. Check errors and actual readbacks after configuration. TekScope's `REMOTE:RUNStop`, `REMOTE:ACQMethod`, DATA/WFMOutpre/CURVe dialect is not implemented by these Rigol-style headers. The documented subset is a statement about the supported commands: a matching vendor header is not proof of equivalent behavior, and an unsupported header reports -113.

The `:SYSTem:SET` key schema is the way to reach settings that have no header; see [setup keys](setup-keys.md). The documentation is checked by `ui/src/scpi_coverage_tests.rs`: every header of the extension tables and every setup key must appear in these files, and no header may appear in two group tables.

See also: [reference analysis](../reference-analysis.md) for sampling, model-derived jitter, physical alignment and C-PHY limits (extensions, not stock Rigol or DPOJET compatibility claims).
