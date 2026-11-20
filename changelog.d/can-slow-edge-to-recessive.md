Fixed:

- The CAN and CAN-FD decoders keep their bit timing on a line that is slow on its way back to recessive: they
  resynchronise on the edges to dominant alone, as a node does. Resynchronised on every edge, they sampled the bit
  behind a slow edge in its neighbour: on CAN_H at 500 kbit/s, back at the threshold 1.1 us late, a frame of every
  cycle had a wrong CRC or a wrong identifier, and frames were missed ([Features](wiki/Features.md)) (unit).
