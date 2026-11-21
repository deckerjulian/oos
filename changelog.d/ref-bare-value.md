Fixed:

- `:REFerence:VSCale 2`, `:VOFFset 0.1`, `:COLor RED`, `:LABel:CONTent` and `:SHOW 1|0` without a slot set the
  selected reference slot, as [docs/scpi.md](docs/scpi.md) says. A value that read like a slot (a number, or a word
  starting with R) was taken for one and refused with -224.
