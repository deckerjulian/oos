# Timebase

The main time base, the delayed (zoom) time base, roll and XY mode. The Studio zoom controls (zoom lane, vertical zoom, fit) are in [display](display.md#studio-zoom).

Menu location: Horizontal (Time/div, Delay, Zoom, Fine time base, Expand around, Timebase mode, Auto roll, XY).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TIMebase[:MAIN]:SCALe` | Set / Query | Seconds/div; nearest supported timebase. | `2.000000E-04` | Horizontal > Time/div | Rigol-compatible |
| `:TIMebase[:MAIN]:OFFSet` | Set / Query | Horizontal delay in seconds; `POSition` is an alias. | `0.000000E+00` | Horizontal > Delay | Rigol-compatible |
| `:TIMebase:DELay:ENABle` | Set / Query | Boolean zoom enable. | `0` | Horizontal > Zoom on/off | Rigol-style (command family of the DHO guide) |
| `:TIMebase:DELay:SCALe` | Set / Query | Zoom seconds/div. | `5.000000E-11` | Horizontal > Zoom time/div | Rigol-style (command family of the DHO guide) |
| `:TIMebase:DELay:OFFSet` | Set / Query | Physical centre time of the zoom, seconds. | `0.000000E+00` | Horizontal > Zoom position | Rigol-style (command family of the DHO guide) |
| `:TIMebase:VERNier` | Set / Query | Boolean fine horizontal control. | `0` | Horizontal > Fine time base | Rigol-style (command family of the DHO guide) |
| `:TIMebase:HREFerence:MODE` | Set / Query | Horizontal expansion reference. | `CENT` | Horizontal > Expand around | Rigol-style (command family of the DHO guide) |
| `:TIMebase:MODE` | Set / Query | Main/zoom/XY mode supported by the model. | `MAIN` | Horizontal > Timebase mode | Rigol-style (command family of the DHO guide) |
| `:TIMebase:ROLL` | Set / Query | Model roll setting; acquisition source restrictions still apply. | `1` | Horizontal > Auto Roll | Rigol-style (command family of the DHO guide) |
| `:TIMebase:XY:ENABle` | Set / Query | Boolean XY display. | `0` | Horizontal > Timebase mode (XY) | Rigol-style (command family of the DHO guide) |
| `:TIMebase:XY:X` | Set / Query | Channel role of the XY axis. | `CHAN1` | Horizontal > XY X / Y source | Rigol-style (command family of the DHO guide) |
| `:TIMebase:XY:Y` | Set / Query | Channel role of the XY axis. | `CHAN2` | Horizontal > XY X / Y source | Rigol-style (command family of the DHO guide) |
| `:TIMebase:XY:Z` | Set / Query | Channel role of the Z input; `NONE` disables it. | `NONE` | Horizontal > Z input | Rigol-style (command family of the DHO guide) |

## Examples

Main time base.

```text
:TIMebase:SCALe 1e-3
:TIMebase:OFFSet 2e-3
:TIMebase:SCALe?                       -> 1.000000E-03
:TIMebase:OFFSet?                      -> 2.000000E-03
```

Delayed (zoom) time base.

```text
:TIMebase:DELay:ENABle ON
:TIMebase:DELay:SCALe 1e-4
:TIMebase:DELay:OFFSet 2e-3
:TIMebase:DELay:SCALe?                 -> 1.000000E-04
:TIMebase:DELay:ENABle OFF
```

