# Setup keys (`:SYSTem:SET` and `:SYSTem:GET?`)

`:SYSTem:SET <key> = <value>` writes a key of the setup file on the live settings; `:SYSTem:GET? <key>` reads it. It is the setup schema: an escape hatch for tests and for the few settings that have no header. Where a header exists (the "Equivalent header" column), use that; the headers are in the [group pages](README.md#command-groups). The file written by Save setup and returned by `:SYSTem:SETup?` is the same list as `key = value` lines (`#` starts a comment), and `*SAV` / `*RCL` use the same files.

- Values: booleans `true`/`false`, numbers in base units, enumerations as the Rust variant name (`Dc`, `Rise`, `Auto`), lists separated by blanks.
- A key that is not in the list is rejected with -224 after the fact. Some keys are only written when they differ from their default and are marked optional.
- Keys that carry a number are written with it (`ch2.vdiv`); here that number is `<n>`.
- The Default column is the value of the default model (the Default setup), as `:SYSTem:SETup?` writes it; a dash means the key is optional or only written for a non-default state.
- A source in a setup key is its token: the channel number (`1` is CH1) or `R1`; `-` or `off` is none.
- Some rows describe several keys with one text where the original description was shared; the keys are listed in the first column in the same order as the values of the second.

## Channels

Commands: [channel.md](channel.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `ch<n>.on` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert | `true` | `:CHANnel<n>:DISPlay` |
| `ch<n>.vdiv` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert | `1` | `:CHANnel<n>:SCALe` |
| `ch<n>.offset` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert | `-1.5` | `:CHANnel<n>:OFFSet` |
| `ch<n>.coupling` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert | `Dc` | `:CHANnel<n>:COUPling` |
| `ch<n>.bw` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert | `Auto` | `:CHANnel<n>:BWLimit` |
| `ch<n>.probe` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert | `1` | `:CHANnel<n>:PROBe` |
| `ch<n>.invert` | on, V/div step, offset (volts), `Dc`/`Ac`/`Gnd`/`Dc50`, bandwidth limit (`Auto`, `Full`, `20 MHz`, ...), probe ratio, invert | `false` | `:CHANnel<n>:INVert` |
| `ch<n>.vfine` | optional: fine scale | - | `:CHANnel<n>:VERNier` |
| `ch<n>.unit` | unit | - | `:CHANnel<n>:UNITs` |
| `ch<n>.bias` | zero bias | - | `:CHANnel<n>:BIAS` |
| `ch<n>.skew` | probe deskew | - | `:CHANnel<n>:TCALibrate` |
| `ch<n>.label` | label | - | `:CHANnel<n>:LABel:CONTent` |
| `active` | the selected channel, 0-based | `0` | `:CHANnel:ACTive` |
| `fine_v` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | `false` | `:CHANnel<n>:VERNier` |
| `v_expand_gnd` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | `false` | - |

## Timebase

Commands: [timebase.md](timebase.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `tdiv` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | `0.0002` | `:TIMebase:SCALe` |
| `delay` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | `0` | `:TIMebase:OFFSet` |
| `fine_t` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | `false` | `:TIMebase:VERNier` |
| `tfine` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | - | - |
| `h_expand` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | `Center` | `:TIMebase:HREFerence:MODE` |
| `auto_roll` | time/div step, delay, fine switches (`tfine` optional), vertical expand about ground, horizontal expand (`Center`, `Left`, `Right`, `Trigger`), auto roll | `true` | `:TIMebase:ROLL` |
| `zoom` | zoom on/off | `false` | `:TIMebase:DELay:ENABle` |
| `zoom_tdiv` | zoom time/div step | `0.00000000005` | `:TIMebase:DELay:SCALe` |
| `zdelay` | zoom delay | `0` | `:TIMebase:DELay:OFFSet` |

## Trigger

Commands: [trigger.md](trigger.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `trig_src` | source channel 0-based; reference slot (0-based) or `none`; logic line or `none` | `0` | `:TRIGger:EDGE:SOURce` |
| `trig_ref` | source channel 0-based; reference slot (0-based) or `none`; logic line or `none` | `none` | - |
| `trig_la` | source channel 0-based; reference slot (0-based) or `none`; logic line or `none` | `none` | - |
| `trig_src2` | source channel 0-based; reference slot (0-based) or `none`; logic line or `none` | `3` | `:TRIGger:SOURce2` |
| `trig_ref2` | source channel 0-based; reference slot (0-based) or `none`; logic line or `none` | `none` | - |
| `trig_level` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `1.5` | `:TRIGger:EDGE:LEVel` |
| `trig_level2` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `0.5` | `:TRIGger:LEVel2` |
| `slope` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `Rise` | `:TRIGger:EDGE:SLOPe` |
| `trig_slope2` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `Rise` | `:TRIGger:SHOLd:SLOPe` |
| `trig_mode` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `Auto` | `:TRIGger:SWEep` |
| `holdoff` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `0` | `:TRIGger:HOLDoff` |
| `trig_type` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `Edge` | `:TRIGger:MODE` |
| `trig_nrej` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `false` | `:TRIGger:NREJect` |
| `trig_coup` | level, level 2, `Rise`/`Fall`/`Either`, sweep `Auto`/`Normal`/`Single`, holdoff, type name, noise reject, coupling | `Dc` | `:TRIGger:COUPling` |
| `trig_pol` | `Pos`/`Neg`; `Off`/`Gt`/`Lt`/`In`/`Out`; time and upper limit | `Pos` | `:TRIGger:PULSe:POLarity` |
| `trig_when` | `Pos`/`Neg`; `Off`/`Gt`/`Lt`/`In`/`Out`; time and upper limit | `Lt` | `:TRIGger:PULSe:WHEN` |
| `trig_t1` | `Pos`/`Neg`; `Off`/`Gt`/`Lt`/`In`/`Out`; time and upper limit | `0.00002` | `:TRIGger:PULSe:WIDTh` |
| `trig_t2` | `Pos`/`Neg`; `Off`/`Gt`/`Lt`/`In`/`Out`; time and upper limit | `0.00004` | `:TRIGger:PULSe:UWIDth` |
| `video_std` | `Pal`/`Ntsc` | `Pal` | `:TRIGger:VIDeo:STANdard` |
| `video_sync` | `AllLines`/`Line`/`Odd`/`Even`/`AllFields` | `AllFields` | `:TRIGger:VIDeo:MODE` |
| `video_line` | line | `10` | `:TRIGger:VIDeo:LINE` |
| `pat` | four bits `H L X R F` | `H X X H` | `:TRIGger:PATTern:PATTern` |
| `pat_levels` | four levels | `1.25 0 1.65 1.65` | `:TRIGger:PATTern:LEVel` |
| `pat_la` | the 16 logic-line bits as one word of `H L X` | `XXXXXXXXXXXXXXXX` | - |
| `win_mode` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value | `Enter` | `:TRIGger:WINDow:POSition` |
| `nth_idle` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value | `0.00005` | `:TRIGger:NEDGe:IDLE` |
| `nth_n` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value | `1` | `:TRIGger:NEDGe:EDGE` |
| `setup` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value | `0.000001` | `:TRIGger:SHOLd:STIMe` |
| `hold` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value | `0.000001` | `:TRIGger:SHOLd:HTIMe` |
| `trig_bus` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value | `ID/Addr` | `:TRIGger:BUS:WHEN` |
| `trig_bus_val` | `Enter`/`Exit`/`Inside`; idle time; edge number; setup and hold times; bus match, bus value | `291` | `:TRIGger:BUS:VALue` |
| `auto_nrej` | the auto set-up sets noise reject from the noise on the flat levels of the trigger signal | `false` | - |
| `ser.rel`, `ser.max` | protocol trigger: relation of the value (`Eq`/`Ne`/`Gt`/`Lt`/`In`/`Out`) and its upper limit | `Eq`, `255` | - |
| `ser.mask` | protocol trigger: the bits of the value that count (SPI, I2C data) | `18446744073709551615` | - |
| `ser.uart_inv`, `ser.uart_stop`, `ser.uart_err` | UART trigger: idle low, stop bits (`One`/`OneHalf`/`Two`), error kind (`Either`/`Parity`/`Framing`) | `false`, `One`, `Either` | - |
| `ser.spi_cs_high`, `ser.spi_timeout` | SPI trigger: chip select active high; without chip select, the clock pause that starts a frame, s (0 = the decoder's) | `false`, `0` | - |
| `ser.addr_rel`, `ser.addr_max`, `ser.dir`, `ser.ten_bit` | I2C trigger: address relation and upper limit, direction (`Either`/`Write`/`Read`), 10-bit addresses | `Eq`, `127`, `Either`, `false` | - |
| `ser.bytes` | I2C trigger: 0 = any data byte of a frame, 1..5 = the first data bytes after the address | `0` | - |

## Acquisition

Commands: [acquire.md](acquire.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `acq` | `Normal`/`Peak`/`Average`/`HiRes` | `Normal` | `:ACQuire:TYPE` |
| `avg_count` | averages | `16` | `:ACQuire:AVERages` |
| `segments` | segments (0 = off) | `0` | `:ACQuire:ULTRa:MAXFrame` |
| `mdepth` | memory depth index | `0` | `:ACQuire:MDEPth` |
| `cores` | the offsets of the interleaved ADC cores taken off in software | `true` | `:ACQuire:CORes` |
| `avg_hw` | Average mode in the FPGA where the board can (not verified on a unit) | `false` | `:ACQuire:AVERages:HARDware` |
| `seg_hw`, `seg_interval` | segments recorded by the FPGA (not verified on a unit); wait after each segment, s | `false`, `0` | `:ACQuire:SEGMents:HARDware`, `:ACQuire:SEGMents:INTerval` |
| `draw_path` | Acquire > Draw path: `standard` or `batch` | `standard` | `:SYSTem:PIPeline` |

## Calibration and deskew

Commands: [calibration.md](calibration.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `deskew_ch<n>` | optional: analysis deskew seconds | - | `:CALibrate:DESKew` |
| `deskew_ref<n>` | optional: analysis deskew seconds | - | `:CALibrate:DESKew` |

## System

Commands: [system.md](system.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `touch_lock` | touch lock | `false` | `:SYSTem:LOCK` |
| `quick` | quick key action | `Screenshot` | `:SYSTem:QUICk` |
| `aux_out` | rear AUX OUT: `TrigOut`/`Off`/`PassFail` | `TrigOut` | `:SYSTem:AOUTput` |

## Display, windows, views

Commands: [display.md](display.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `text_size` | text size | `Normal` | `:DISPlay:TEXTsize` |
| `hdmi` | HDMI mode | `Waveform` | `:DISPlay:HDMI` |
| `hdmi_view` | Display > Monitor view: the view an HDMI monitor shows (0: the panel's) | `0` | `:DISPlay:VIEW:MONitor` |
| `bar` | Display > Bottom bar: `cards` or `compact` | `cards` | - |
| `persist` | display settings | `0` | `:DISPlay:GRADing:TIME` |
| `intensity` | display settings | `60` | `:DISPlay:WBRightness` |
| `grid` | display settings | `Full` | - |
| `grid_bright` | display settings | `100` | `:DISPlay:GBRightness` |
| `grid_frame` | display settings | `true` | - |
| `color_grade` | display settings | `false` | `:DISPlay:COLor` |
| `dots` | display settings | `false` | `:DISPlay:TYPE` |
| `windows` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `One` | - |
| `density` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `Scope` | - |
| `split_pane` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `0` | - |
| `ch_window` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `1 1 1 1` | - |
| `math_window` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `1 1 1 1 1 1 1 1` | - |
| `la_window` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `1` | - |
| `ref_window` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `1 1 1 1 1 1 1 1 1 1` | - |
| `pane` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `0` | - |
| `trend_sel` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `0 0` | - |
| `strip` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `Below` | - |
| `meas_labels` | window layout; `ch_window`, `math_window`, `ref_window` hold one window number per slot (1-based) | `false` | `:MEASure:INDicator` |
| `layout` | layout name; the Studio keys (`studio_dock`, `studio_ov`, `studio_meas_view` = `detailed`, `studio_badges` = `classic` are optional) | `minimal` | `:DISPlay:LAYout` |
| `studio_view` | layout name; the Studio keys (`studio_dock`, `studio_ov`, `studio_meas_view` = `detailed`, `studio_badges` = `classic` are optional) | `waveform` | - |
| `studio_dock` | layout name; the Studio keys (`studio_dock`, `studio_ov`, `studio_meas_view` = `detailed`, `studio_badges` = `classic` are optional) | - | - |
| `studio_ov` | layout name; the Studio keys (`studio_dock`, `studio_ov`, `studio_meas_view` = `detailed`, `studio_badges` = `classic` are optional) | - | - |
| `studio_meas_view` | layout name; the Studio keys (`studio_dock`, `studio_ov`, `studio_meas_view` = `detailed`, `studio_badges` = `classic` are optional) | - | - |
| `studio_badges` | layout name; the Studio keys (`studio_dock`, `studio_ov`, `studio_meas_view` = `detailed`, `studio_badges` = `classic` are optional) | - | - |
| `view` | the analyser view (`Scope`, `Spectrum`, `Eye`, ...) | `Scope` | `:VIEW` |
| `views` | `views` is `<count> <current, 1-based>` | `1 1` | `:DISPlay:VIEW:COUNt` |
| `view<n>` | `view<n>` is what view n keeps while another is shown: `key=value; ...` of the measurement list, panels, cursors, zoom, lanes shown and analysis set-ups, values escaped with `%3B` for `;` (optional) | - | - |
| `math_hidden` | math channels (1-based) not shown in the view shown (optional) | - | `:MATH<n>:DISPlay` |
| `gen_hidden` | `true`: the GEN chip is not shown in the view shown (optional) | `false` | `:DISPlay:GENerator` |
| `studio_state` | what the Ui keeps of the view shown: pinned measurements, panel mode, search, zoom source (optional; written each time) | empty | - |

## Measurements

Commands: [measure.md](measure.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `meas` | rows as `<Item>:<source>`, blank separated, for example `Vpp:1` or `Vpp:R2` (the source token is the channel number, 1 = CH1, or `R<n>`) | `` | `:MEASure:ITEM` |
| `meas2` | pair rows as `<pair number>:<source A>:<source B>` | `` | `:MEASure:ITEM` |
| `cat` | optional: catalogue rows `<id>:<A>:<B>:<C>:<D>` | - | `:MEASure:CATalog:ITEM` |
| `row_cfg` | optional: per-row settings, `<row key>\\|<settings text>`, blank separated; implementation specific | - | - |
| `meas_cfg` | `<idle> <Above/Below/Both> <N periods> <data edges 0/1> <low pct> <high pct> <Tsetup> <Thold>` | `0.00005 Above 6 0 0.1 0.9 0 0` | `:MEASure:BURSt:IDLE ...` |
| `meas_levels` | `<absolute 0/1> <custom 0/1> <mid> <hysteresis> <low V> <mid V> <high V> <hysteresis V> <separate fall 0/1> <fall low> <fall mid> <fall high> <fall low V> <fall mid V> <fall high V> <base/top method>` | `0 0 0.5 0.05 -1 0 1 0.03 0 0.1 0.5 0.9 -1 0 1 Auto` | `:MEASure:LEVel:...` |
| `meas_stat` | optional: `<current 0/1> <sigma 0/1> <min 0/1> <max 0/1>` | - | `:MEASure:STATistic:...` |
| `meas_full_record` | gate: whole record or the screen | `true` | `:MEASure:GATing` |

## Cursors, DVM, counter

Commands: [cursor.md](cursor.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `cursor` | cursor mode, positions (fractions of the screen), second cursor source (optional) | `Off` | `:CURSor:MODE` |
| `cur_a` | cursor mode, positions (fractions of the screen), second cursor source (optional) | `0.3` | - |
| `cur_b` | cursor mode, positions (fractions of the screen), second cursor source (optional) | `0.7` | - |
| `cur_c` | cursor mode, positions (fractions of the screen), second cursor source (optional) | `0.3` | - |
| `cur_d` | cursor mode, positions (fractions of the screen), second cursor source (optional) | `0.7` | - |
| `cur_b_source` | cursor mode, positions (fractions of the screen), second cursor source (optional) | - | - |
| `dvm` | `<on> <source> <mode>` | `false 0 0` | `:DVM:ENABle ...` |
| `counter` | `<on> <source> <mode> <digits>` | `false 0 0 5` | `:COUNter:ENABle ...` |

## Data logger

Commands: [logger.md](logger.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `log_ivl` | logger interval, index into 1, 2, 5, 10, 30, 60, 300, 600, 1800, 3600 s | `0` | `:SYSTem:LOG:INTerval` |

## Math

Commands: [math.md](math.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `math` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `Off` | `:MATH<n>:OPERator` |
| `math_a` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `0` | `:MATH<n>:SOURce1` |
| `math_b` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `1` | `:MATH<n>:SOURce2` |
| `math_scale` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `3` | `:MATH<n>:SCALe` |
| `math_fc` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `10000` | - |
| `math_lin` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `1 0` | - |
| `math_offset` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `0` | `:MATH<n>:OFFSet` |
| `fft_win` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `Hann` | `:MATH<n>:FFT:WINDow` |
| `math_bw` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `2000` | - |
| `math_sel` | the selected math channel (`math_sel`, 0-based): operator, sources, scale index, corner, `<A> <B>` of A x + B, offset, FFT window, bandwidth | `0` | - |
| `m<n>.op` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `Off` | - |
| `m<n>.a` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `0` | - |
| `m<n>.b` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `1` | - |
| `m<n>.scale` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `3` | - |
| `m<n>.fc` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `10000` | - |
| `m<n>.bw` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `2000` | - |
| `m<n>.offset` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `0` | - |
| `m<n>.win` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `Hann` | - |
| `m<n>.lin` | the same for math channels 2 to 8 (the selected one is written as `math*`) | `1 0` | - |
| `math_expr<n>` | optional: the formula text of channel n | - | `:MATH<n>:EXPRession` |

## Spectrum

Commands: [spectrum.md](spectrum.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `spec_margin` | spectrum limit margin, dB | `10` | `:SPECtrum:LIMit:MARGin` |
| `spec` | source token, centre Hz, span Hz (0 = full), RBW Hz, window name, trace mode index, ref level dBV, waterfall 0/1, THD/SNR 0/1 (the last two as `true`/`false`) | `1 0 0 0 Blackman 0 10 false false` | `:SPECtrum:SOURce ...` |
| `spec_peaks` | peak sort index, harmonics index | `0 2` | `:SPECtrum:PEAK:HARMonics` |
| `spec_acpr` | channel width Hz, channel spacing Hz | `1000000 2000000` | `:SPECtrum:ACPR:WIDTh ...` |

## Decoders and bus

Commands: [decode.md](decode.md).

The keys of the selected decoder are plain (`dec_*`); decoders 2 and up repeat them with a prefix: `d<n>.on`, `d<n>.src`, and so on. `dec_opt` is four numbers whose meaning depends on the protocol.

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `dec_sel` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `0` | `:DECode:SELect` |
| `dec_on` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `false` | `:DECode:STATe` |
| `dec_proto` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `Uart` | `:DECode:PROTocol` |
| `dec_src` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `2` | `:DECode:SOURce` |
| `dec_src2` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `3` | `:DECode:SOURce2` |
| `dec_cs` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `1` | `:DECode:CS` |
| `dec_src4` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `none` | `:DECode:SOURce4` |
| `dec_diff` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `none` | `:DECode:DIFFerential` |
| `dec_thr` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `1.65` | `:DECode:THReshold` |
| `dec_ascii` | selected decoder, on, protocol name, role sources (index; `none`), threshold, ASCII or hex | `true` | `:BUS<n>:FORMat` |
| `dec_baud` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `7` | `:BUS<n>:RS232:BAUD` |
| `dec_canrate` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `2` | - |
| `dec_canh` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `false` | - |
| `dec_fdrate` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `1` | - |
| `dec_arate` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `0` | - |
| `dec_tick` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `0` | - |
| `dec_nibbles` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `6` | - |
| `dec_uspeed` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `0` | - |
| `dec_cphy_rate` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `0` | `:DECode:CPHY:RATE` |
| `dec_mode` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `0` | - |
| `dec_bits` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `8` | `:BUS<n>:RS232:DBITs` |
| `dec_parity` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `None` | `:BUS<n>:RS232:PARity` |
| `dec_opt` | protocol parameters: ladder indices (baud, CAN, FD, ARINC, SENT tick, USB speed), `dec_opt` is four numbers whose meaning depends on the protocol | `0 0 0 0` | - |
| `dec_la` | `<input 0/1> <16 logic line numbers or ->` | `0 8 10 - -` | - |
| `bus_on` | the plot-bus (I2C lane) | `false` | `:BUS:LANE:STATe` |
| `bus_label` | the plot-bus (I2C lane) | `I2C` | `:BUS:LANE:LABel` |
| `bus_pos` | the plot-bus (I2C lane) | `-3` | `:BUS:LANE:POSition` |
| `bus_kind` | the plot-bus (I2C lane) | `I2c` | - |
| `bus_clk` | the plot-bus (I2C lane) | `1` | `:BUS:LANE:CLOCk:SOURce` |
| `bus_data` | the plot-bus (I2C lane) | `2` | `:BUS:LANE:DATA:SOURce` |
| `bus_clk_thr` | the plot-bus (I2C lane) | `1.65` | `:BUS:LANE:CLOCk:THReshold` |
| `bus_data_thr` | the plot-bus (I2C lane) | `1.65` | `:BUS:LANE:DATA:THReshold` |
| `bus_rw` | the plot-bus (I2C lane) | `false` | `:BUS:LANE:RW` |

## Logic analyser

Commands: [logic.md](logic.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `la` | the logic analyser: `<on> <line mask hex> <...> <threshold pod 1> <threshold pod 2> <...>` as `la = false ffff 0 0 1.65 1.65 1` | `false ffff 0 0 1.65 1.65 1` | `:LA:STATe ...` |

## Eye and jitter

Commands: [eye.md](eye.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `eye` | source, bit rate Hz, from-record 0/1 | `1 0 0` | `:EYE:SOURce ...` |
| `eye_more` | BER, PN low Hz, PN high Hz, NPJ 0/1, rail source or `-`, notch Hz, clock (strobe) source or `-`, high-pass 0/1 | `0.000000000001 1000 1000000 0 - 0 - 1` | `:EYE:BER ...` |
| `eye_pll` | optional PLL bandwidth Hz (only when above 0) | - | `:EYE:PLL:BWIDth` |
| `eye_place` | `Full`/`Side`/`Under` | `Full` | `:EYE:PLACe` |

## Power, magnetic, inverter, double pulse

Commands: [power.md](power.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `power` | voltage source index, current source index, page name | `1 2 Quality` | `:POWer:VOLTage:SOURce ...` |
| `power_more` | line f Hz, switch f Hz, input source or `-`, reference volts or `-`, clock source or `-` | `0 0 - - -` | `:POWer:FLINe ...` |
| `magnetic` | voltage source, current source, turns, Ae, Le, Rcu, plot (`IPsi`/`BH`), loop cursor | `1 2 1 0 0 0 IPsi 0` | `:MAGnetic:VOLTage:SOURce ...` |
| `inverter` | page (`Dq0`/`Mech`), phase A, B, C source, angle source name, poles, analog source, volts-mode 0/1, torque source or `off`, bus voltage source or `off`, bus current source or `off`, analog scale, degrees per volt, offset, electrical 0/1, encoder line A, encoder line B, index line or `off`, counts per revolution, swap 0/1, Hall line A, B, C | `Dq0 1 2 3 Off 1 4 true off off off 1 1 0 false 0 1 off 1024 false 0 1 2` | `:INVerter:PAGE ...` |
| `dpt` | Vds, Id, Vgs source tokens, fourth channel mode (`Off`/`Gate`/`DiodeV`/`DiodeI`), fourth source, gate percent, zoom (`Both`/`TurnOn`/`TurnOff`) | `1 2 3 Off 4 10 Both` | `:DPT:VDS:SOURce ...` |

## Bode

Commands: [bode.md](bode.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `bode` | start Hz, stop Hz, points, amplitude V, input source, output source, generator index, generator address `ip:port` | `100 1000000 40 1 1 2 0 192.168.1.100:5555` | `:BODE:STARt ...` |
| `bode_link` | `lan`/`usb` | `lan` | `:BODE:GENerator` |
| `bode_mode` | `Loop`/`Psrr`/`Impedance` | `Loop` | `:BODE:MODE` |
| `bode_sweep` | free sweep: time s, log true/false, sync channel (1-based), sync edge (`Rise`/`Fall`), sync level V, input channel (0 = none), reference dBV | `1 true 4 Rise 1 1 0` | `:BODE:SWEep:TIME ...` |

## SIM

Commands: [sim.md](sim.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `sim` | preset index, slot, rate b/s, PRBS order, high V, low V, rise s, noise V, bandwidth Hz, de-emphasis dB, spread 0/1, random jitter s, periodic jitter s, periodic jitter Hz, reserved 0 | `3 0 16000000000 31 0.5 -0.5 0.00000000002 0.005 12000000000 4.5 1 0.0000000000012 0.000000000004 8000000 0` | `:SIM:PRESet ...` |
| `sim_extra` | spread centre true/false, spread deviation, spread frequency Hz, DCD s, correlated RJ s, correlated corner Hz | `false 0.005 33000 0 0 1000000` | `:SIM:SSC:MODE ...` |

## Mask, zones, histogram

Commands: [mask.md](mask.md).

| Key | Value | Default | Equivalent header |
| --- | --- | --- | --- |
| `zone<n>` | `<on> <source, 1-based> <inside true/false> <t:v points, blank separated>` | `false 1 true` | `:ZONE<n>:STATe ...` |
| `z_src` | Z input channel or `off` | `off` | - |
| `hist` | histogram type | `Off` | `:HISTogram:TYPE` |
| `mask_on` | mask test settings (see also `:MASK:...`) | `false` | `:MASK:ENABle` |
| `mask_src` | mask test settings (see also `:MASK:...`) | `0` | `:MASK:SOURce` |
| `mask_x` | mask test settings (see also `:MASK:...`) | `0.2` | `:MASK:X` |
| `mask_y` | mask test settings (see also `:MASK:...`) | `0.4` | `:MASK:Y` |
| `mask_stop` | mask test settings (see also `:MASK:...`) | `false` | `:MASK:OUTPut` |
| `mask_inv` | mask test settings (see also `:MASK:...`) | `false` | `:MASK:INVert` |

## Keys without a header

These menu rows stay without a header: pure chrome (window layout, density, strip position, dock arrangement and the Studio panels, which stay on `studio_*`), keypad and picker navigation, the candidate pick of Identify, joining a WLAN (it needs a password: do it on the screen), and the per-row measurement settings card (`row_cfg`). Measurement catalogue rows are added with `:MEASure:CATalog:ITEM` or the `cat` key, and `:MEASure:READout?` exports them as `CAT<id>`.

## Examples

```text
:SYST:SET tdiv = 5                       (time/div step)
:SYST:GET? tdiv
:SYST:SET ch1.coupling = Ac
:SYST:SET eye = R2 0 1                   (source R2, bit rate 0, from record)
:SYSTem:SETup?                           (the whole setup as an IEEE block)
```
