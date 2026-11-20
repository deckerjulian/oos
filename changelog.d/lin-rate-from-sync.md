Fixed:

- The LIN decoder reads a bus at a rate that Decode > Baud does not list: it measures the bit time of each frame on
  its sync field, as a node does, and Baud is only for a record without a readable sync. A frame whose sync is not
  0x55 is listed with its sync as the error, not left as a break alone, and a byte that comes later than a frame may
  last is no longer taken for its checksum. Found on a capture of a real bus at 18350 bit/s, which read nothing but
  breaks at every Baud but 19200 ([Features](wiki/Features.md)).
