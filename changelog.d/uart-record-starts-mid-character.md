Fixed:

- The UART decoder lists no false first characters when a record begins inside a character: the first falling edge
  of the record was taken for a start bit, and with characters back to back the wrong framing ran on (FF, ED in front
  of a 9600 Bd text line). Up to two real characters at the very start of such a record are left out rather than
  guessed (unit) ([Features](wiki/Features.md)).
