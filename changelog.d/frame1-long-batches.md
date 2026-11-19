Fixed:

- At short time bases, no stray trace now and then from a record joined with older memory before the trigger (a step
  about 38 us left of the trigger at 10 us/div, 1 kpts). The first record after each arm of a long batch is left out,
  which costs one record of 26 to 32 (unit)
  ([docs/protocols/acquisition-start.md](docs/protocols/acquisition-start.md)).
