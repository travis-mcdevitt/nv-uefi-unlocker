# MSI TU102 22 GiB: one completed guarded boot and Windows tensor validation

One locally guarded build completed its preboot transaction on an MSI RTX
2080 Ti 22 GiB and returned to Windows unattended. In that Windows session,
FP16-input, FP32-accumulating native MMA instructions ran at approximately
1021 FLOP/SM/clock, matching the FP16-accumulation control. This is functional
evidence of near-full-rate execution for this boot, not a direct post-driver
MMIO readback of the selector registers.

There was **one successful boot and one approximately 14-second compute suite**.
There is no matched stock baseline, measured percentage speedup, repeated-boot
validation, or endurance result. A full power cycle immediately before the
successful attempt was not independently established. This report does not
claim a controlled cold-boot comparison.

## Hardware and firmware identity

| Item | Observed value |
| --- | --- |
| GPU | MSI RTX 2080 Ti, TU102, 68 SMs, compute capability 7.5 |
| PCI vendor/device | `10de:1e07` |
| Subsystem vendor/device | `1462:371d` |
| Capacity | 22528 MiB / 22 GiB |
| VBIOS | `90.02.42.00.35` |
| Native FWSEC code / data / total | `0x9a00 / 0x3f0 / 0x9df0` bytes |
| Native FWSEC SHA-256 | `d54e1512845a091631abf374a0b748a226203bb0cf9ee4d7b0316023a957ea6f` |
| Exact exported PCI view SHA-256 | `2fd04d5696dee29d4fb7d4bed72a3120eead2180b77f02e6008cc674f4115c4f` |
| Windows driver / GSP firmware | `610.88 / 610.57.01` |

The FWSEC hash covers unmodified IMEM followed by unmodified DMEM. This is a
distinct firmware profile from the `.e9` profile discussed in PR 49. Firmware
bytes and the original ROM are not included. The overlay validates the exact
PCI-view and FWSEC hashes before its active transaction; a hash match alone
is not a claim of compatibility on another board.

## What ran

The tested EFI was **local guarded-auto build 004**, derived from
[`3bb069bf5783d3a95d8f1ba545b41224e102defc`](https://github.com/physicsdolphin/nv-uefi-unlocker/tree/3bb069bf5783d3a95d8f1ba545b41224e102defc).
Its SHA-256 was
`73cbe014f3feb6b927301fb624d21832aa56cd7264ba519909042bb82be4252a`
(232448 bytes). It was not the unmodified release binary and was not PR 49.

The local policy fixed the SEC2 C-reference path: entry HALT, no SEC2 reset,
TRANSCFG unchanged, portal readback verification, then a three-second interval
without MMIO after START. PTIMER seeding and host-probe writes were disabled.
The build also required exact identity/ROM/asset checks, high DMA staging,
already-enabled bridge bus mastering, FWSEC WPR bounds plus GSP HALT, explicit
DMA ownership, and a consumed one-use permission. Failures after detach stopped
without retry, reset, reconnect, or OS handoff. The successful path skipped FLR
and reconnect.

This is evidence that the combined guarded C-load configuration can succeed
without PTIMER seeding or host-probe writes on this board. It does **not** isolate
which C-load difference was necessary, prove equivalence to an unmodified
upstream experiment, or show that any guard alone caused the success.

## Three attempts, with local guard failures attributed correctly

| Attempt | Result | Evidence boundary |
| --- | --- | --- |
| 001 | C-load portal checks and START completed; local post-booter guard entered STOP | The local guard compared PLM to `0xffffffff` through a helper that rejects that value. The predicate was impossible; individual post-booter reads were not logged, so actual PLM at that point is unknown. |
| 002 | Post-booter mask `0x00`; selector writes read back `0x88888888 / 0x8`; local final guard entered STOP | Relock readback was `0xffffffef`, while the local final guard demanded `0xffffff8f`. The combined final expression did not log every read, so the exact failed clause and possible additional failures are unproven. |
| 003 | Post-booter and final masks `0x00`; all six DMA releases succeeded; Windows handoff observed | Build 004 added complete final diagnostics and accepted exactly `0xffffff8f` or `0xffffffef`; other guard requirements remained. |

Both earlier STOP predicates were introduced in this local guarded candidate.
They must not be presented as two upstream loader failures. The separate
upstream all-ones early-check issue should be assessed on its own source and
regression evidence.

Attempt 003's final snapshot was:

```text
GSP_CPUCTL=0x00000010  SEC2_CPUCTL=0x00000010
FEAT_PLM=0xffffffef   SS0=0x88888888  SS1=0x00000008
WPR2_HI=0x00000000   failure_mask=0x00
PCI_COMMAND=0x0003 (matches entry)
stock signature pointer=0x100020000, size=0x1000 (both match)
DMA release: booter/signature/meta/stock/boot/radix=true
FINAL: transaction complete, all DMA released
CHAIN: LoadImage (FromDevicePath)
```

The log intentionally closes before LoadImage. The operator's unattended
return observation and subsequent Windows checks corroborated the handoff;
the LoadImage log line alone is not proof of a running OS. Windows reported
the NVIDIA device OK, configuration-manager error code 0, 22528 MiB and active
GSP firmware.

## Windows functional and numerical checks

| Native SM75 instruction | Median FLOP/SM/clock | Median TFLOP/s |
| --- | ---: | ---: |
| FP16 inputs, FP32 accumulation | 1021.3603 | 93.0879 |
| FP16 inputs, FP16 accumulation | 1020.9939 | 93.0931 |

The rate ratio was `1.0003589`. Each mode used two warmups and seven interleaved
measured launches, 1024 threads per block and one block on each of 68 SMs.
The explicit `m16n8k8` MMA loop used four independent accumulators and 2048
rounds. Per-block `clock64` intervals were retained. Every launch recorded 68
unique SM IDs, and every accumulator component ended at exactly 8 after
nonzero cancelling arithmetic. The operation count includes the final
nonzero check: `(2048 * 64 + 4) * 32 * 2048` FLOPs per SM.

| cuBLAS inputs / accumulation / output | Square matrix N | Median TFLOP/s | Relative L2 error |
| --- | ---: | ---: | ---: |
| FP16 / FP32 / FP32 | 4096 | 78.0412 | 6.30e-6 |
| FP16 / FP32 / FP32 | 8192 | 79.9715 | 5.64e-6 |
| FP16 / FP16 / FP16 control | 4096 | 83.5547 | 0.00484 |
| FP16 / FP16 / FP16 control | 8192 | 83.6120 | 0.00336 |
| Ordinary FP32 control | 4096 | 12.6579 | 9.08e-7 |

The target cases explicitly selected `CUBLAS_COMPUTE_32F`, FP32 output and
`DISALLOW_REDUCED_PRECISION_REDUCTION`. Buffers were preallocated; inputs were
nonzero and seeded. Seven interleaved windows per case each exceeded 250 ms
of CUDA-event time. All three modes passed a complete 256x256 CPU-FP64 product
comparison. Each large case passed 32 CPU-FP64 dot-product samples, not a
complete comparison of every large output. Target/FP32 checks used a `5e-5`
relative-L2 limit; FP16 controls used `0.01`. Different output types mean the
GEMM control ratio is not a pure instruction-rate or application-speedup ratio.

PyTorch `2.9.1+cu130`, NVRTC `13.0` and cuBLAS `13.0.0` were used. Peak sampled
temperature was 60 C. The power limit stayed at 250 W; peak sampled board power
was 261.74 W. The suite exited 0, with no failed numerical checks or matching
NVIDIA/display/WHEA/Kernel-PnP warning/error/critical events in its short
observation interval. No privileged MMIO driver was installed for the test.

The instruction benchmark is adapted from the
[pinned native tensor test](https://github.com/haohervchb/Turing-unlock/blob/a188b24a392414f29f931ea886329227cfbc2f98/tools/tensor_baseline.cu),
under [copyleft-next-0.3.1](https://github.com/haohervchb/Turing-unlock/blob/a188b24a392414f29f931ea886329227cfbc2f98/LICENSE).
The original source SHA-256 is
`2b93e0e7e5ea74a52b3aaa5c3666b3a5ac031eb002330abbdb8c8648f4c611ba`.
Exact hashes of the measured harness, generated CUDA and PTX are retained in
[portable-results.json](portable-results.json).

## Diagnostics still needing interpretation

`EXCI=0x00900100` remains in all three C-load post-quiet readouts, including the
successful one. No exception-free execution claim is made, nor is the value
declared harmless. Under the bit range `[24:20]` proposed in issue 17,
`(0x00900100 >> 20) & 0x1f` equals `9`, not `0x10`; the actual cause mapping and
whether the value is stale need authoritative confirmation.

The generic `FACTORY FEAT_READOUT_1=0xbadf5040` line samples `0x823818`.
The Turing PLM/selector observations here are from the `0x4096xx` block.
Consequently, that generic line's parenthetical “PLM still closed” cannot
override the successful Turing readbacks or establish a speed verdict.
Its original wording is preserved in the excerpts so the diagnostic problem
is visible.

The observed relock word `0xffffffef` is not the full factory word
`0xffffff8f`: bits 5/6 differ. NVIDIA's pinned
[TU102 display-block PLM definitions](https://github.com/NVIDIA/open-gpu-doc/blob/9fdf5c4062007929d9f4e6cbad9c9771fe61b880/manuals/turing/tu102/dev_display_withoffset.ref.txt#L1121-L1136)
assign write levels 0/1/2 to bits 4/5/6. Applying that layout to FEAT is an
inference, supported by observed transitions and the
[existing TU106 regression](https://github.com/physicsdolphin/nv-uefi-unlocker/blob/3bb069bf5783d3a95d8f1ba545b41224e102defc/docs/evidence/tu106-pr13-regression/notes.md),
not a published FEAT specification. The report does not establish why bits
5/6 remained set.

## Evidence and source reconstruction

- [Attempt 001 excerpts](attempt-001-excerpt.log), [attempt 002 excerpts](attempt-002-excerpt.log), and [attempt 003 excerpts](attempt-003-excerpt.log) retain every `BOOTER-C:` line, original line numbers, and the original full-log hashes.
- [Portable results](portable-results.json) retain instruction samples, per-SM cycles, matrix timing/correctness, relevant versions and hashes through an explicit field allowlist.
- [Source overlay manifest](source-overlay-manifest.json) identifies the 20 files over the pinned base. The original hardware-tested and publication hashes are both recorded. Publication replaces one operator-specific documentation PATH command and translates one source comment into English; executable code, runtime strings, Cargo manifests/config and Cargo.lock are unchanged. The publication copy was not rerun on hardware.
- [Reconstruction instructions](REPRODUCE.md) describe source assembly and host tests. The complete historical host suite passed 116 tests with private fixtures; the default suite leaves two fixture tests ignored.
- [Source license](source-LICENSE.txt): GPL-2.0-only. The benchmark's separate provenance/license is retained by its own contribution.

This is an experimental source/evidence contribution, not a binary boot kit.
It includes no ROM, firmware, payload blobs, EFI binary, execution markers,
USB deployment helper, or boot-configuration script. Raw OS inventory,
registry and event dumps are omitted. Operator names, local absolute paths,
serials, GPU UUIDs, boot GUIDs, hostnames, network identifiers, absolute capture
timestamps and timezone are excluded. Public repository URLs and source
attribution remain. Source comments inherited from upstream retain their
public historical dates; they are not capture timestamps.

The hardware result is already recorded in the [issue 17 update](https://github.com/physicsdolphin/nv-uefi-unlocker/issues/17#issuecomment-6515965775). This archive supplies its supporting evidence and experimental source; it is not an additional hardware result.
