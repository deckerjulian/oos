# Data logger

Log the measurement strip to a CSV file at a fixed interval.

Menu location: Data logger (Logging on/off, Logging interval, Log file path / state).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SYSTem:LOG` | Set / Query | Log on/off (query added; the older set form is the same) | `0` | Data logger > Logging | OOS extension |
| `:SYSTem:LOG:INTerval` | Set / Query | Seconds, the nearest of 1, 2, 5, 10, 30, 60, 300, 600, 1800, 3600 | `1.000000E+00` | Data logger > Interval | OOS extension |
| `:SYSTem:LOG:FILE` | Query | Path of the CSV file while logging, else empty | - | Data logger > Log file | OOS extension |
| `:SYSTem:LOG:INFO` | Query | The info row, for example `log-001.csv: 12 rows` | `6 measurements, off` | Data logger > Log info | OOS extension |

## Examples

Log the measurements every 5 s.

```text
:SYSTem:LOG:INTerval 5
:SYSTem:LOG:INTerval?                  -> 5.000000E+00
:SYSTem:LOG ON
:SYSTem:LOG:FILE?                      -> /tmp/oos-scpiext-712316/log-001.csv
:SYSTem:LOG:INFO?                      -> log-001.csv: 0 rows
:SYSTem:LOG OFF
```

