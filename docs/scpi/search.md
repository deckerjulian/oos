# Search (mark events)

Mark the events of a trigger-like condition and walk through them. The first set of a source, level or slope adds Search 1 like the Add rail does. Only an edge search exists. The decoded-event table has its own commands in [decode](decode.md).

Menu location: Decode and bus > Mark events, Search (source, slope, level, 50 %, from / to trigger, event, event list, save events, remove).

Part of the [SCPI reference](README.md).

## Commands

| Command | Set/Query | Parameters and range | Default | Menu equivalent | Notes |
| --- | --- | --- | --- | --- | --- |
| `:SEARch:STATe` | Set / Query | Mark the events of the trigger condition | `0` | Trigger > Mark events | name not verified against the DHO guide |
| `:SEARch:MODE` | Set / Query | `EDGE` (the only kind) | `EDGE` | - | name not verified against the DHO guide |
| `:SEARch:EDGE:SOURce` | Set / Query | Search source; without a search of its own the trigger's. The first set of a source, level or slope adds Search 1 like the Add rail does | `CHAN1` | Decode and bus > Search source | name not verified against the DHO guide |
| `:SEARch:EDGE:SLOPe` | Set / Query | `POSitive`, `NEGative`, `RFALl` | `POS` | Decode and bus > Search slope | name not verified against the DHO guide |
| `:SEARch:EDGE:LEVel` | Set / Query | Volts | `1.500000E+00` | Decode and bus > Search level | name not verified against the DHO guide |
| `:SEARch:EDGE:LEVel:AUTO` | Action | Level to the middle of the source | - | Decode and bus > Level 50 % | name not verified against the DHO guide |
| `:SEARch:COPY:TO` | Action | `TRIGger` `TRIGger`: copy the search to the trigger. | - | Decode and bus > To trigger | name not verified against the DHO guide |
| `:SEARch:COPY:FROM` | Action | `TRIGger` `TRIGger`: copy the trigger to the search. | - | Decode and bus > From trigger | name not verified against the DHO guide |
| `:SEARch:COUNt` | Query | Number of events in the record shown | `1` | - | name not verified against the DHO guide |
| `:SEARch:EVENt` | Set / Query | Go to event `1`..count (the view moves there); the query answers the current one | `1` | Decode and bus > Event | name not verified against the DHO guide |
| `:SEARch:DATA` | Query | Rows `time,source #n,type,ok`, one per line, like `:DECode:EVENts?` | - | Decode and bus > Event list | name not verified against the DHO guide |
| `:SEARch:SAVE` | Action | Save the events as CSV (`events-*.csv` in the save folder) | - | Decode and bus > Save events | name not verified against the DHO guide |
| `:SEARch:REMove` | Action | Remove Search 1 | - | Decode and bus > Remove | name not verified against the DHO guide |

## Examples

Mark rising edges and walk through them.

```text
:STOP
:SEARch:EDGE:SOURce CHAN1
:SEARch:EDGE:SLOPe POSitive
:SEARch:EDGE:LEVel:AUTO
:SEARch:STATe ON
:SEARch:COUNt?                         -> 1
:SEARch:EVENt 1
:SEARch:EVENt?                         -> 1
```

Copy the trigger condition into the search and back.

```text
:SEARch:COPY:FROM TRIGger
:SEARch:EDGE:SOURce?                   -> CHAN1
:SEARch:COPY:TO TRIGger
:SEARch:REMove
```

