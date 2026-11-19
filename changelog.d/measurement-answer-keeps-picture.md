Fixed:

- The channel traces no longer drop out of single pictures while the scope runs with measurements that are computed in
  the background: the answer of such a measurement cleared the picture until the next record came, so on a DHO924S a
  third of the pictures had no traces, and with persistence the collected picture started over (unit)
  ([Measurements and Analysers](wiki/Measurements-and-Analysers.md#measurements)).
