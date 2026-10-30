# Reconstruct the experimental source snapshot

This directory contains a source overlay over the exact upstream base
`3bb069bf5783d3a95d8f1ba545b41224e102defc`, not a standalone checkout.
The tested artifact was local guarded-auto build 004. Applying these files to
another revision, or building the base revision alone, does not reproduce that
source. `source-overlay-manifest.json` records original and published hashes.

The publication overlay differs from the preserved hardware-tested snapshot
in two nonfunctional places: an operator-specific PATH command is removed
from the historical documentation, and one Chinese comment in
`loader/src/tx_fwsec.rs` is translated into English. Executable code, runtime
strings, Cargo manifests/config and Cargo.lock are unchanged. The manifest
retains both the original tested file hashes and the publication hashes. No
new hardware test was performed for this publication copy. The historical
document describes the state before attempt 003; the evidence README
describes the later result.

Assemble a separate checkout using Git and PowerShell. The example assumes
the shell starts in this evidence directory and the destination is new:

```powershell
$evidenceRoot = (Get-Location).Path
$rebuildRoot = Join-Path (Split-Path $evidenceRoot -Parent) 'guarded-rebuild'
git clone --no-checkout https://github.com/physicsdolphin/nv-uefi-unlocker $rebuildRoot
git -C $rebuildRoot checkout --detach 3bb069bf5783d3a95d8f1ba545b41224e102defc
Get-ChildItem -LiteralPath (Join-Path $evidenceRoot 'source-overlay') -Force |
    Copy-Item -Destination $rebuildRoot -Recurse -Force
```

Verify each overlay path against `published_sha256` before building. The
remaining files come from the base commit. The source retains its exact
hardware, PCI-location, ROM and asset pins; they are evidence of the tested
configuration, not parameters validated for other machines.

Historical host-test command, with Rust/Cargo available on PATH:

```powershell
Set-Location -LiteralPath $rebuildRoot
cargo +1.98.1-x86_64-pc-windows-msvc xt --locked
```

This runs only host crates and tests. The original suite had 116 passes with
the private ROM and regenerated firmware fixtures. Without fixtures, the
`exported_rom_fixture` and `regenerated_asset_fixtures` tests remain explicitly
ignored; their absence must not be reported as validation of those inputs.
To reproduce all fixture checks, the same byte-identical private ROM is needed
through `TU102_ROM` and the exact locally regenerated assets through
`TU102_ASSETS`; neither is distributed here. Then the recorded command is:

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc xt --locked -- --include-ignored
```

The historical EFI compilation command for the selected feature is:

```powershell
cargo +1.98.1-x86_64-pc-windows-gnu build --release -p loader --target x86_64-unknown-uefi --features guarded-auto --locked
```

Default, ordinary `tx`, `guarded-msi` and `guarded-auto` release configurations
all compiled in the original build validation. Those compile results are not
additional hardware tests. QEMU/OVMF testing was not performed.

The unchanged base `loader/build.rs` embeds the build's current local timestamp
and timezone. Consequently, reconstruction of the tested logic is supported, but the
publication comment translation and current build timestamp mean a rebuild
is **not promised to match the original EFI SHA-256**. The original EFI hash is
an artifact identity, not a reproducible-build claim. No EFI is distributed.

The seven required runtime asset names, sizes and SHA-256 pins remain in
`guard/src/lib.rs`; the source base includes the upstream extraction tooling.
No firmware, original ROM, decrypted/generated payload, execution marker,
USB-preparation script, firmware boot entry or restart helper is included.
These instructions perform source assembly/testing/compilation only.

For performance reproduction, the native method is pinned to
`haohervchb/Turing-unlock@a188b24a392414f29f931ea886329227cfbc2f98`,
`tools/tensor_baseline.cu`. The portable JSON preserves measured Python,
CUDA-source and PTX hashes and the exact runtime versions. A subsequently
generalized Windows benchmark contribution is a distinct source revision;
matching methodology must not be represented as rerunning the measured
source byte-for-byte. The observed Windows results are in the evidence README.

The source overlay follows the base repository's GPL-2.0-only license; see
`source-LICENSE.txt`. The separately attributed benchmark uses
copyleft-next-0.3.1. No license transfer between those codebases is implied.
