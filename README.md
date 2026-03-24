# rustc miscompilation: `str == str` wrong on mips64 big-endian bare-metal target

## Summary

When targeting a custom `mips64-unknown-none` target (big-endian MIPS64, bare-metal),
the Rust compiler miscompiles `str` equality. The generated assembly is missing the
conditional branch on the `memcmp`/`bcmp` return value, so `str == str` always
evaluates to `false`.

The same code compiles correctly for `mips64-unknown-linux-gnuabi64`.

## Prerequisites

A nightly Rust toolchain with `rust-src`:

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

## Reproducing

Build and emit assembly:

```bash
RUSTFLAGS="-Clink-arg=-e_start -Cllvm-args=-mno-check-zero-division" \
  cargo +nightly rustc -Zbuild-std=core -Zjson-target-spec \
    --target ./mips64-unknown-none.json \
    --release -- --emit=asm
```

Extract the `_start` function:

```bash
sed -n '/_start:/,/\.end _start/p' target/mips64-unknown-none/release/deps/mips_repro-*.s
```

## What to look for

The generated `_start` should contain:
1. A call to `memcmp` or `bcmp`
2. A **conditional branch** on the return value (e.g., `beqz $2, ...`)
3. Two exit paths: `exit(0)` for equal, `exit(1)` for not-equal

### Broken output (mips64-unknown-none)

```asm
_start:
    ...
    bne     $6, $1, .LBB1_2     # if lengths differ, jump to LBB1_2
    nop
    jal     memcmp               # call memcmp (lengths matched)
    ld      $5, 8($sp)
.LBB1_2:
    addiu   $1, $zero, 1         # exit code = 1 (WRONG — no branch on memcmp result)
    addiu   $2, $zero, 5058
    move    $4, $1
    syscall                      # exit(1) unconditionally
```

The `beqz` branch on `memcmp`'s return value is missing. The `exit(0)` path does
not exist. Both the length-mismatch and length-match paths converge to `exit(1)`.

### Correct output (mips64-unknown-linux-gnuabi64)

For comparison, building for the standard Linux MIPS64 target:

```bash
RUSTFLAGS="-Clink-arg=-e_start -Cllvm-args=-mno-check-zero-division" \
  cargo +nightly rustc -Zbuild-std=core \
    --target mips64-unknown-linux-gnuabi64 \
    --release -- --emit=asm
```

```asm
_start:
    ...
    bne     $6, $1, .LBB1_2     # if lengths differ, jump to exit(1)
    nop
    jalr    $25                  # call bcmp (lengths matched)
    ld      $5, 0($sp)
    beqz    $2, .LBB1_3         # if bcmp == 0 (equal), jump to exit(0)  <-- THIS IS MISSING
    nop
.LBB1_2:
    addiu   $1, $zero, 1        # exit(1) — strings not equal
    ...syscall...
.LBB1_3:
    addiu   $1, $zero, 0        # exit(0) — strings equal
    ...syscall...
```

## Source

```rust
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let a: &str = core::hint::black_box("name");
    let b: &str = core::hint::black_box("name");
    if a == b {
        exit(0) // expected path
    } else {
        exit(1) // bug: this is the only path emitted for mips64-unknown-none
    }
}
```

`core::hint::black_box` prevents the compiler from constant-folding the comparison
at the call site, forcing it to emit the actual comparison logic.

## Affected versions

Tested and confirmed broken on:
- `nightly-2025-07-01` (rustc 1.90.0-nightly)
- `nightly-2025-08-01` (rustc 1.90.0-nightly)
- `nightly-2026-01-15`
- `nightly-2026-02-20`
- `nightly-2026-03-22` (rustc 1.96.0-nightly)

Note: older nightlies require minor target spec adjustments (`"target-pointer-width"`
as string `"64"` instead of integer `64`, and no `"abi"` field). A compatible spec
for older nightlies is included as `mips64-unknown-none-old.json`.

## Impact

All `str` comparisons (`==`, `!=`, `match` on `&str`) produce wrong results on
this target. This breaks serde deserialization (field name matching), string
lookups, and any code that compares strings. `[u8]` slice comparisons are not
affected — only `str` equality.
