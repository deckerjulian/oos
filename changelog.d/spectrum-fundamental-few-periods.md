Fixed:

- The spectrum page and `:SPECtrum:ITEM? F1` find the fundamental of a square or a ramp with few periods on the
  screen: a 10 kHz square at 50 us/div read 16.2 kHz, a ramp 12.3 kHz, and THD, SNR, SINAD, SFDR and ENOB were
  figures of the wrong lines. The window of the distortion figures follows the periods in the record (unit)
  ([Measurements and Analysers](wiki/Measurements-and-Analysers.md#spectrum)).
