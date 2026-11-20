Fixed:

- The UART decoder lists no false first characters when a record begins inside a character: the first falling edge
  of the record was taken for a start bit, and with characters back to back the wrong framing ran on (FF, ED in front
  of a 9600 Bd text line). Up to four real characters at the very start of such a record are left out rather than
  guessed; a record that begins on the idle line a few bit times before a character of a regular stream keeps it
  (unit) ([Features](wiki/Features.md)).
