Removed:

- Builds without some decoders: `OOS_DECODERS`, `OOS_DECODERS_OFF` and the Cargo features `dec-*` are gone, and every
  decoder is always in oos-ui, as it was in every release image. System > Features leaves decoders out at run time
  ([protocol-modules.md](docs/protocol-modules.md#leaving-decoders-out), ADR 0071).
