**What changes, and why:**

**Checks:** which layers of `tools/check.sh` ran, and where it was tested: on a unit (which model), only on the host,
or not at all.

**Register writes:** `tools/plan-matrix.sh` before and after: the same / they differ, because ...

Please confirm (CONTRIBUTING.md, ADR 0026):

- [ ] Every commit is signed off (`git commit -s`): the work is mine, or I have the right to submit it, under
      GPL-2.0-or-later.
- [ ] No vendor material in the commits, their messages or this text: no code, listings, tables, strings or names
      from the stock or a modified firmware. The hardware is described as behaviour, with how it is known.
- [ ] No serial numbers, MAC addresses, licence keys or addresses of a scope.
- [ ] Every hardware fact the code uses is in `docs/` as behaviour, with how it is known (ADR 0030).
- [ ] A new or superseding decision record has the status `proposed` (ADR 0031).
- [ ] Code or text from another project is named here with its source and licence.
