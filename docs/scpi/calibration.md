# Calibration and deskew

Self-calibration is a workflow, not a passive diagnostic read: `:CALibrate:STARt` needs the appropriate disconnected-input setup and the hardware source. The analysis deskew corrects the time alignment of channels and references for analysis; native transfer metadata is unchanged. It is not the probe deskew `:CHANnel<n>:TCALibrate`.

Menu location: System > Self-calibration; Channels > Auto deskew, Clear deskew, Deskew.

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:CALibrate:DESKew:AUTO` | Action | Line up the shown traces near the view | - | System > Auto deskew | OOS extension |
| `:CALibrate:DESKew:CLEar` | Action | Clear the analysis deskew | - | System > Clear deskew | OOS extension |
| `:CALibrate:DESKew` | Set / Query | `<wave>,<seconds>` Analysis deskew of a channel or reference (the query takes `<wave>`); this is not the probe deskew `:CHANnel<n>:TCALibrate` | `0.000000E+00` | schema keys `deskew_ch<n>`, `deskew_ref<n>` | OOS extension |
| `:CALibrate:STARt` | Action | `<0..4>[,ZERO][,ADC]` (extended) `ADC` also calibrates the ADC cores | - | Self-calibration ADC cores | OOS extension |
| `:CALibrate:STATus` | Query | Self-calibration status text. | `needs the hardware source` | System > Self-calibration (channels, AFE zero, start, cancel, status) | OOS extension |
| `:CALibrate:CANCel` | Action | Cancel a running self-calibration. | - | System > Self-calibration (channels, AFE zero, start, cancel, status) | OOS extension |

## Examples

Status and analysis deskew.

```text
:CALibrate:STATus?                     -> needs the hardware source
:CALibrate:DESKew CHAN2,1e-9
:CALibrate:DESKew? CHAN2               -> 1.000000E-09
:CALibrate:DESKew:CLEar
```

