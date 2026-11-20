Fixed:

- An update that brings another device tree starts by kexec again when the loader seeded the kernel's address
  randomisation: the seed is carried into the new tree as 0, which kexec-tools requires (issue #193; host test; the
  developer's unit writes no seed, so it has not run on a unit).
