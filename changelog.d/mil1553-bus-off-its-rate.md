Fixed:

- The MIL-STD-1553 decoder reads a bus a few percent off its rate: it follows the edge in the middle of every bit
  from bit to bit. It sampled the 17 bits of a word on a 1 us grid from the sync, and on a bus 4 % slow the later
  bits fell into the wrong half: on a capture of a real bus 37 of 71 words were misread or had a parity error, now 2
  (the capture's own faults) ([Features](wiki/Features.md)).
