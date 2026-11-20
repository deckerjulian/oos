Fixed:

- After STOP the record on screen is the newest one the hardware took, and it stays the record measured and read
  out: a record of the whole memory is no longer replaced by a view read again about 90 ms later, and `*OPC?` after
  `:STOP` answers once the record of the STOP is in place, so `:WAV:PREamble?` and `:WAV:DATA?` describe the same
  record ([SCPI and VXI-11](wiki/SCPI-and-VXI-11.md)).
