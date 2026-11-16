# Waveform transfer

Read waveform points and their scaling over SCPI. `:WAVeform:SOURce` selects the source for transfer and does not select the acquisition trigger. For a channel, NORMal reads the interpolated display record and RAW the retained native memory (when stopped); a reference uses its native arrays in all three modes. Do not assume `:ACQuire:SRATe?` describes a reference: read its preamble.

Menu location: none (data transfer); Display and layout > Screenshot for `:DISPlay:DATA?` (see [display](display.md)).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:WAVeform:SOURce` | Set / Query | Select `CHANn\|Rn` for transfer; does not select the acquisition trigger. | `CHAN1` | none (data transfer) | Rigol-compatible |
| `:WAVeform:MODE` | Set / Query | `NORMal\|MAXimum\|RAW`. MAX uses the CH screen while running and native memory while stopped. | `NORM` | none (data transfer) | Rigol-compatible |
| `:WAVeform:FORMat` | Set / Query | `BYTE\|WORD\|ASCii`. Invalid formats are rejected without changing the selection. | `BYTE` | none (data transfer) | Rigol-compatible |
| `:WAVeform:STARt` | Set / Query | One-based inclusive start of the transfer page. Setting either bound keeps start <= stop. | `1` | none (data transfer) | Rigol-compatible |
| `:WAVeform:STOP` | Set / Query | One-based inclusive end of the transfer page; default through the last available point. | `3600` | none (data transfer) | Rigol-compatible |
| `:WAVeform:POINts` | Set / Query | Query actual points in the page; set the requested count starting at STARt. Pages clip at the available data boundary. | `3600` | none (data transfer) | Rigol-compatible |
| `:WAVeform:PREamble` | Query | Ten comma-separated fields listed below. | - | none (data transfer) | Rigol-compatible |
| `:WAVeform:DATA` | Query | IEEE definite-length binary block; ASCII format also uses a block. | - | none (data transfer) | Rigol-compatible |

## Preamble and formats

`:WAVeform:PREamble?` returns ten comma-separated fields:

```text
format,type,points,count,xincrement,xorigin,xreference,yincrement,yorigin,yreference
```

`format` is 0 BYTE, 1 WORD, 2 ASCII; `type` is the selected mode (0 NORM, 1 MAX, 2 RAW); `count` is 1. Integer data converts as

```text
time[k]  = (k - xreference) * xincrement + xorigin
volts[k] = (code[k] - yorigin - yreference) * yincrement
```

BYTE is unsigned 8-bit. WORD is unsigned **little-endian** 16-bit, an explicit OOS wire policy (Rigol does not specify the byte order, so this is not a verified Rigol claim). ASCII is comma-separated volts in seven-significant-digit scientific notation; do not scale it again. `:WAVeform:DATA?` returns an IEEE definite-length block `#<N><N digits of length><payload>` followed by a newline: read the announced length, never split on newlines inside a payload. Pages whose worst-case encoded size exceeds 64 MiB are refused with -223. Stop acquisition first when metadata and data must match.

A minimal Python reader is in the [README](README.md#reading-a-waveform-in-python).

## Examples

Read the preamble of a stopped channel.

```text
:STOP
:WAVeform:SOURce CHAN1
:WAVeform:MODE NORMal
:WAVeform:FORMat BYTE
:WAVeform:STARt 1
:WAVeform:STOP 100
:WAVeform:POINts?                      -> 100
:WAVeform:PREamble?                    -> 0,0,100,1,5.555555E-07,-1.000000E-03,0,3.200000E-02,-47,128
```

Choose a reference and word format.

```text
:WAVeform:SOURce R1
:WAVeform:SOURce?                      -> R1
:WAVeform:FORMat WORD
:WAVeform:FORMat?                      -> WORD
```

