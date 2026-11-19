Fixed:

- The I2C decoder no longer calls the last byte of a read an error: a controller ends every read by not acknowledging
  its last byte, so on a working bus every read was red on the band and an Error (NACK) event of a search. That byte
  is still marked `N`; a byte not acknowledged anywhere else stays an error ([Features](wiki/Features.md)).
