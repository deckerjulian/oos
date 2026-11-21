Fixed:

- With a display persistence time (100 ms, 500 ms and longer) the trace no longer flickers or pulses at slow time
  bases and deep memory. The picture faded on every screen refresh, also while it waited for the next record, so the
  newest trace faded out before the next one came. It now fades when a new record arrives, by the time since the last
  one: the newest trace stays at full brightness until the next record replaces it
  ([First Steps](wiki/First-Steps.md)).
