# Power, magnetic, inverter and double pulse

The four power-electronics analysers. Each has a view (`:VIEW POWER|MAGNETIC|INVERTER|DPT`), a source and parameter setup, and a figure readout `:<page>:ITEM?`. The figure queries take the source tokens `CHANn|Rn` after the item. Magnetic figures need the configured geometry and calibration. These extensions are not stock Rigol compatibility claims.

Menu location: Power, Magnetic, Inverter and Double pulse (view on, sources, parameters, figures).

Part of the [SCPI reference](README.md).

## Commands

### Power

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:POWer:VOLTage:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH1` | Power > Voltage | name not verified against the DHO guide |
| `:POWer:CURRent:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH2` | Power > Current | name not verified against the DHO guide |
| `:POWer:VIN:SOURce` | Set / Query | Source `CHANn\|Rn`, or `OFF`. | `OFF` | Power > Input | name not verified against the DHO guide |
| `:POWer:CLOCk:SOURce` | Set / Query | Source `CHANn\|Rn`, or `OFF`. | `OFF` | Power > Clock | name not verified against the DHO guide |
| `:POWer:PAGE` | Set / Query | Option text of the Page row | `QUALITY` | Power > Page | name not verified against the DHO guide |
| `:POWer:FLINe` | Set / Query | Hz. Range 0.0 to 1e9. | `0.000000E+00` | Power > Line f | name not verified against the DHO guide |
| `:POWer:FSWitch` | Set / Query | Hz. Range 0.0 to 1e9. | `0.000000E+00` | Power > Switch f | name not verified against the DHO guide |
| `:POWer:REFerence` | Set / Query | Volts; 0 means none. | `0.000000E+00` | Power > Reference | name not verified against the DHO guide |
| `:POWer:ITEM` | Query | `<item>[,V,I]` `VRMS`, `IRMS`, `REAL`, `APP`, `REAC`, `PF`, `PHAS`, `FREQ`, `EON`, `EOFF`, `ECOND`, `RDSON`; single-source ripple items also exist. | - | Power > Read power figures and harmonics | OOS extension |
| `:POWer:HARMonics` | Query | `[source]` Semicolon-separated `index,amplitude` pairs; `9.9E37` when none. | `9.9E37` | Power > Read power figures and harmonics | OOS extension |

### Magnetic

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:MAGnetic:VOLTage:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH1` | Magnetic > Voltage | OOS extension |
| `:MAGnetic:CURRent:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH2` | Magnetic > Current | OOS extension |
| `:MAGnetic:TURNs` | Set / Query | Number of turns. Range 1.0 to 10000.0. | `1.000000E+00` | Magnetic > Turns | OOS extension |
| `:MAGnetic:AREA` | Set / Query | Core area Ae, m^2. Range 0.0 to 1e3. | `0.000000E+00` | Magnetic > Area | OOS extension |
| `:MAGnetic:LENGth` | Set / Query | Path length Le, m. Range 0.0 to 1e3. | `0.000000E+00` | Magnetic > Length | OOS extension |
| `:MAGnetic:RCU` | Set / Query | Copper resistance, ohms. Range 0.0 to 1e6. | `0.000000E+00` | Magnetic > Rcu | OOS extension |
| `:MAGnetic:PLOT` | Set / Query | `I-PSI` or `B-H` (dashes optional); the query answers `I-PSI` or `B-H`. | `I-PSI` | Magnetic > Plot | OOS extension |
| `:MAGnetic:CURSor` | Set / Query | Loop point index. Range 0 to 1000000. | `0` | Magnetic > Cursor | OOS extension |
| `:MAGnetic:ITEM` | Query | `<item>[,V,I]` `L`, `B`, `H`, `MUR`, `LOSS`. Needs the configured geometry and calibration. | `9.9E37` | Magnetic > Read L, B, H, MUR, LOSS | OOS extension |

### Inverter

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:INVerter:PAGE` | Set / Query | `DQ0`, `MECHANICAL` | `DQ0` | Inverter > Page | OOS extension |
| `:INVerter:PHASe1:SOURce` | Set / Query | Phase source `CHANn\|Rn`. | `CH1` | Inverter > Phase A | OOS extension |
| `:INVerter:PHASe2:SOURce` | Set / Query | Phase source `CHANn\|Rn`. | `CH2` | Inverter > Phase B | OOS extension |
| `:INVerter:PHASe3:SOURce` | Set / Query | Phase source `CHANn\|Rn`. | `CH3` | Inverter > Phase C | OOS extension |
| `:INVerter:ANGLe:SOURce` | Set / Query | `OFF`, `PHASE`, `ANALOG`, `QUAD`, `HALL` | `OFF` | Inverter > Angle | OOS extension |
| `:INVerter:POLEs` | Set / Query | 1 to 32 Range 1 to 32. | `1` | Inverter > Poles | OOS extension |
| `:INVerter:ANALog:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH4` | Inverter > Analog | OOS extension |
| `:INVerter:ANALog:SCALe` | Set / Query | Scale. Range 1e-3 to 1e3. | `1.000000E+00` | Inverter > Analog | OOS extension |
| `:INVerter:ANALog:DEGRees` | Set / Query | Degrees per volt. Range 1e-3 to 1e4. | `1.000000E+00` | Inverter > Analog | OOS extension |
| `:INVerter:ANGLe:OFFSet` | Set / Query | Degrees, +-3600 Range -3600.0 to 3600.0. | `0.000000E+00` | Inverter > Offset | OOS extension |
| `:INVerter:QUANtity` | Set / Query | `VOLTAGE`, `CURRENT` | `VOLTAGE` | Inverter > Signal | OOS extension |
| `:INVerter:TORQue:SOURce` | Set / Query | Source or `OFF`. | `OFF` | Inverter > Torque | OOS extension |
| `:INVerter:BUS:VOLTage:SOURce` | Set / Query | Source or `OFF`. | `OFF` | Inverter > Bus V | OOS extension |
| `:INVerter:BUS:CURRent:SOURce` | Set / Query | Source or `OFF`. | `OFF` | Inverter > Bus I | OOS extension |
| `:INVerter:ELECtrical` | Set / Query | `MECH`, `ELEC` | `MECH` | Inverter > Mech / Elec | OOS extension |
| `:INVerter:ENCoder:CPR` | Set / Query | Counts per revolution. Range 1.0 to 100000.0. | `1.024000E+03` | Inverter > Encoder | OOS extension |
| `:INVerter:ENCoder:SWAP` | Set / Query | Boolean; swap A and B. | `0` | Inverter > Encoder | OOS extension |
| `:INVerter:ENCoder:A` | Set / Query | Logic line 0 to 15. Range 0 to 15. | `0` | Inverter > Encoder | OOS extension |
| `:INVerter:ENCoder:B` | Set / Query | Logic line 0 to 15. Range 0 to 15. | `1` | Inverter > Encoder | OOS extension |
| `:INVerter:HALL1:LINE` | Set / Query | Logic line 0 to 15. Range 0 to 15. | `0` | Inverter > Hall lines | OOS extension |
| `:INVerter:HALL2:LINE` | Set / Query | Logic line 0 to 15. Range 0 to 15. | `1` | Inverter > Hall lines | OOS extension |
| `:INVerter:HALL3:LINE` | Set / Query | Logic line 0 to 15. Range 0 to 15. | `2` | Inverter > Hall lines | OOS extension |
| `:INVerter:ENCoder:INDex` | Set / Query | Encoder index line 0 to 15 or `OFF` | `OFF` | Inverter > Index line | OOS extension |
| `:INVerter:ITEM` | Query | `<item>[,sources...]` Configured inverter result, numeric or text. | - | Inverter > Read inverter figures | OOS extension |

### Double pulse

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:DPT:VDS:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH1` | Double pulse > Vds | OOS extension |
| `:DPT:ID:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH2` | Double pulse > Id | OOS extension |
| `:DPT:VGS:SOURce` | Set / Query | Source `CHANn\|Rn`. | `CH3` | Double pulse > Vgs | OOS extension |
| `:DPT:FOURth` | Set / Query | `OFF`, `GATE`, `DIODE_V`, `DIODE_I`. | `OFF` | Double pulse > Fourth | OOS extension |
| `:DPT:FOURth:SOURce` | Set / Query | Source of the fourth channel. | `CH4` | Double pulse > Channel | OOS extension |
| `:DPT:GATE` | Set / Query | Percent, 1 to 49. Range 1.0 to 49.0. | `1.000000E+01` | Double pulse > Gate | OOS extension |
| `:DPT:ZOOM` | Set / Query | `BOTH`, `TURN-ON`, `TURN-OFF`. | `BOTH` | Double pulse > Zoom | OOS extension |
| `:DPT:ITEM` | Query | `<item>[,V,I[,G]]` `VPEAK`, `IPEAK`, `EON`, `EOFF`, `TDON`, `TR`, `TDOFF`, `TF`, `QRR`. | `9.9E37` | Double pulse > Read double-pulse figures | OOS extension |

## Examples

Power analysis sources.

```text
:POWer:VOLTage:SOURce CHAN1
:POWer:CURRent:SOURce CHAN2
:POWer:FLINe 50
:POWer:FSWitch 100e3
:POWer:VOLTage:SOURce?                 -> CH1
:POWer:FSWitch?                        -> 1.000000E+05
```

Magnetic core parameters.

```text
:MAGnetic:TURNs 20
:MAGnetic:AREA 1e-4
:MAGnetic:LENGth 0.05
:MAGnetic:PLOT BH
:MAGnetic:TURNs?                       -> 2.000000E+01
:MAGnetic:PLOT?                        -> B-H
```

Double pulse.

```text
:DPT:VDS:SOURce CHAN1
:DPT:ID:SOURce CHAN2
:DPT:GATE 10
:DPT:ZOOM BOTH
:DPT:GATE?                             -> 1.000000E+01
:DPT:ZOOM?                             -> BOTH
```

