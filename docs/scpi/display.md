# Display, views and Studio zoom

Screen appearance, the analyser view selector `:VIEW`, the user views, the screenshot transfer and the Studio zoom lane. Display configuration is not proof of hardware acquisition capability.

Menu location: Display and layout (Dots / vectors, Persistence, Waveform intensity, Grid, Colour grade, Backlight, HDMI, Text size, Layout, Graphics, Views, Screenshot) and Run control > Select analyser view.

Part of the [SCPI reference](README.md).

## Commands

### Display and layout

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:DISPlay:DATA` | Query | `[BMP\|PNG]` IEEE block containing a screenshot; default 24-bit BMP. | - | Display > Screenshot | Rigol-compatible |
| `:DISPlay:TYPE` | Set / Query | `DOTS\|VECTors`. | `VECT` | Display > Dots | Rigol-style (command family of the DHO guide) |
| `:DISPlay:GRADing:TIME` | Set / Query | `MIN`, `INFinite`, or a duration mapped to the supported persistence ladder. | `MIN` | Display > Persistence | Rigol-style (command family of the DHO guide) |
| `:DISPlay:WBRightness` | Set / Query | Waveform brightness, percent. | `60` | Display > Intensity | Rigol-style (command family of the DHO guide) |
| `:DISPlay:GBRightness` | Set / Query | Grid brightness, percent. | `100` | Display > Grid bright. | Rigol-style (command family of the DHO guide) |
| `:DISPlay:GRID` | Set / Query | Supported full/half/no-grid display choice. | `FULL` | Display > Grid | Rigol-style (command family of the DHO guide) |
| `:DISPlay:RULers` | Set / Query | Rulers on/off. | `1` | Display > Grid frame | Rigol-style (command family of the DHO guide) |
| `:DISPlay:COLor` | Set / Query | Color grading on/off. | `0` | Display > Colour grade | Rigol-style (command family of the DHO guide) |
| `:DISPlay:BACKlight` | Set / Query | Percent, 10 to 100 | `100` | Display > Backlight | name not verified against the DHO guide |
| `:DISPlay:HDMI` | Set / Query | Option text of the menu row. | `WAVEFORM` | Display > HDMI | OOS extension |
| `:DISPlay:TEXTsize` | Set / Query | Option text of the menu row. | `NORMAL` | Display > Text size | OOS extension |
| `:DISPlay:LAYout` | Set / Query | Option text of the menu row. | `MINIMAL` | Display > Layout | OOS extension |
| `:DISPlay:LOWer` | Set / Query | Option text of the menu row (lower pane; content of the trend chart). | `TRACES` | Display and layout > Lower, Trend chart | OOS extension |
| `:DISPlay:TRENd1` | Set / Query | Option text of the menu row (lower pane; content of the trend chart). | `AUTO` | Display and layout > Lower, Trend chart | OOS extension |
| `:DISPlay:TRENd2` | Set / Query | Option text of the menu row (lower pane; content of the trend chart). | `AUTO` | Display and layout > Lower, Trend chart | OOS extension |

### Views

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:VIEW` | Set / Query | `SCOPE\|SPECTRUM\|EYE\|POWER\|BODE\|MAGNETIC\|INVERTER\|DPT`. | `SCOPE` | Run control > Select analyser view | OOS extension |
| `:DISPlay:VIEW` | Set / Query | The view shown, `1` to the count (Display > Views). | `1` | Run control > Select analyser view | OOS extension |
| `:DISPlay:VIEW:COUNt` | Set / Query | The number of views, 1 to 4. | `1` | Display > Views | OOS extension |

### Studio zoom

Studio only: the zoom lane, its vertical zoom and the fit and centre actions.

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:TIMebase:DELay:SOURce` | Set / Query | The lane that zooms: `ALL`, `CHANn`, `Rn`, `MATHn`; turns the zoom on | `ALL` | Horizontal (Studio zoom) > Zoom source | OOS extension |
| `:TIMebase:DELay:VZOom` | Set / Query | Vertical zoom factor 1 to 1024, the nearest power of two | `1.000000E+00` | Horizontal (Studio zoom) > Y zoom +/- | OOS extension |
| `:TIMebase:DELay:VZOom:RESet` | Action | Vertical zoom back to 1 | - | Horizontal (Studio zoom) > Reset Y | OOS extension |
| `:TIMebase:DELay:FIT` | Action | The zoom window back to the whole screen | - | Horizontal (Studio zoom) > Fit record | OOS extension |
| `:TIMebase:DELay:CENTer` | Action | The trigger in the middle of the zoom window | - | Horizontal (Studio zoom) > Center trigger | OOS extension |

## Examples

Persistence, brightness and grid.

```text
:DISPlay:TYPE VECTors
:DISPlay:GRADing:TIME INFinite
:DISPlay:WBRightness 60
:DISPlay:GBRightness 30
:DISPlay:TYPE?                         -> VECT
:DISPlay:WBRightness?                  -> 60
```

Views.

```text
:DISPlay:VIEW:COUNt 2
:DISPlay:VIEW 2
:DISPlay:VIEW?                         -> 2
:VIEW SPECtrum
:VIEW?                                 -> SPECTRUM
:VIEW SCOPe
```

Studio zoom.

```text
:TIMebase:DELay:SOURce ALL
:TIMebase:DELay:VZOom 4
:TIMebase:DELay:VZOom?                 -> 4.000000E+00
:TIMebase:DELay:VZOom:RESet
:TIMebase:DELay:FIT
```

