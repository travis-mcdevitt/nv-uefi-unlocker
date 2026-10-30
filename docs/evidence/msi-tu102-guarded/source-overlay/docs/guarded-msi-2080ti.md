# Experimental guarded MSI RTX 2080 Ti candidate

This opt-in build adds a fail-closed entry for the local MSI 1462:371d,
10de:1e07 card at 0000:26:00.0, using the user's 90.02.42.00.35 ROM export.
It is an offline-built research candidate, not a hardware-validated release.
The original reference checkouts remain unchanged.

## Guarded behavior

- Requires exactly one NVIDIA display function, the fixed PCI identity/BDF,
  readable D0 power state, memory decode already enabled, expected TU102 chip
  ID and 22 GiB capacity encoding, zero WPR2 high and halted GSP/SEC2 engines.
  Failed config/MMIO reads do not become valid zero values.
- Requires bridge bus mastering already enabled along the target path. This
  build does not enable or restore upstream bridge command bits.
- Uses the bounded NVGI/PCI parser and SHA-256 pins for the complete normalized
  PCI ROM view and native FWSEC image. The validated ROM is cached before
  detach; the active transaction does not reread/reparse an unvalidated PROM.
- Requires all seven assets at their pinned sizes and hashes. File sizes are
  rejected before allocation. No firmware or generated payload enters git.
- Requires `guarded-msi-run.txt` containing exactly the bytes
  `MSI-371D-90.02.42.00.35-SEC2-A`, plus `tx-sec2-c-load.txt`, and Space/Enter
  confirmation. Timeout aborts. Alternate experiment markers are rejected.
  The separate `guarded-auto` feature consumes the exact one-use
  `guarded-auto-once.txt` permission before a 10-second abortable countdown.
  Missing or unsuccessfully consumed permission prevents automatic execution.
- Fixes the SEC2 C-reference experiment. Disables pre-FLR, force-continue,
  low staging, host-probe, timer-seed, VRAM metadata, selector-skip, broad GOP
  disconnect and reconnect alternatives. Allocations must succeed above 4 GiB.
- Requires exact FWSEC WPR bounds plus GSP HALT before success. FWSEC DMA is
  released only after a readable HALT; uncertain/active failures retain it.
- Requires both engines halted and payload readbacks before modifying metadata,
  then verifies a supported host-write-disabled PLM, exact selectors, zero WPR2
  high, stock signature pointer/size and restored PCI command before handoff.
  Final register reads retain transport errors and log every field plus a
  failure mask. Command, signature metadata and each DMA release result are
  logged separately. Release failures are fatal.
- Once detach begins, any transaction failure stops permanently for manual full
  power-off. No emergency FLR, automatic retry, reconnect or OS handoff follows.
  DMA allocations have no implicit Drop release. Panic also stops without an
  automatic shutdown/reset (this panic behavior applies to all candidate builds).
- Resolves exactly one existing Windows Boot Manager before the transaction.
  It performs no controller reconnect scan and has no Linux/removable fallback.
  A returned/failed handoff after a transaction also stops for manual power-off.

## Build 004: final PLM check correction

Attempt 002 passed the post-booter checks and read back SS0=0x88888888 and SS1=8.
The existing write of 0xffffff8f to FEAT PLM read back 0xffffffef, with both
selectors retained. The old final guard required literal 0xffffff8f and stopped.
Its short-circuit expression did not record fresh values for each final check;
therefore the PLM mismatch was observed, but the exact final failed clause and
any simultaneous failures were not independently established.

The corrected predicate accepts exactly 0xffffff8f and 0xffffffef. It rejects
all other words, including 0xffffffff, zero, BADF poison and failed reads. Both
accepted words clear bit 4. The existing relock write is unchanged, and no new
GPU writes, resets, probes, firmware changes or retries were added.

Evidence for this bounded change:

- NVIDIA's [TU102 PLM definitions](https://github.com/NVIDIA/open-gpu-doc/blob/9fdf5c4062007929d9f4e6cbad9c9771fe61b880/manuals/turing/tu102/dev_display_withoffset.ref.txt#L1121-L1136)
  define write permission at levels 0/1/2 as bits 4/5/6. These are display-block
  definitions; applying the encoding to FEAT is an inference supported by the
  observed register transitions, not an exact published FEAT specification.
- The pinned upstream TU106 regression at revision
  3bb069bf5783d3a95d8f1ba545b41224e102defc records 0xffffffef, retained selectors
  and normal Windows handoff in `docs/evidence/tu106-pr13-regression/notes.md`.
  The same word was observed on this TU102 in local attempt 002.
- Pinned PR49 writes 0xffffff8f and reports the result without demanding that
  exact readback. No primary source was found establishing why the host write
  leaves bits 5/6 enabled; a privilege restriction is a hypothesis.

0xffffffef is **not full factory-mask restoration**: bits 5/6 remain enabled.
The policy requires host write protection with an exact observed/reference
word; it does not silently equate those two register states. Windows handoff
and performance on this TU102 remain unverified until a separate hardware run.

Seven new tests cover the recorded open-to-host-closed transition, both accepted
words, all other low-byte values and upper-bit corruption, all 64 combinations
of failed final checks, read errors/poison, selector bit changes and complete
single-read capture despite failures. They run the same final predicate as EFI.

## Original offline verification

Using Rust 1.98.1, the MSVC host suite passes 101 tests: 20 asset tests,
60 existing simulator tests and 21 guard tests. The guard suite includes the
private exported ROM and locally regenerated asset fixtures. The tests cover
truncated/malformed ROMs, bad pointers, duplicate descriptors, identity/warm
entry rejection, invalid policy, per-asset corruption, WPR-before-HALT,
HALT-without-WPR, read failures and timeout behavior. Ten existing Python ROM
tests also pass.

Default, ordinary `tx`, and `guarded-msi` release EFI variants build for
`x86_64-unknown-uefi`. Only the guarded variant has this policy.
Host tests exercise pure policy/mapper/simulator behavior; they do not emulate
the complete UEFI service environment or prove firmware execution on a GPU.

Build from this checkout:

```powershell
# Put the pinned Rust/Cargo toolchain on PATH using your normal setup.
cargo +1.98.1-x86_64-pc-windows-gnu efi-guarded --locked
```

Host fixtures stay outside the checkout. To run all host tests, including them,
set `TU102_ROM` to the exported ROM and `TU102_ASSETS` to the locally regenerated
asset-fixtures directory, then run:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc xt --locked -- --include-ignored
```

## Current evidence boundary

The original build report above is historical. Two separately authorized
hardware attempts now exist outside this checkout in `runs/hardware-attempt-001`
and `runs/hardware-attempt-002`. Both stopped before Windows handoff; the second
established successful selector readback. Neither established a performance
improvement. QEMU/OVMF smoke testing has not been run.

Build 004 is an offline correction to the final policy and diagnostics.
Its validation logs, binary hashes and source snapshot are recorded outside the
checkout in `runs/guarded-build-004`. USB deployment and any further hardware
attempt remain distinct from that offline validation and require coordination
under the user's hardware-change boundary.
