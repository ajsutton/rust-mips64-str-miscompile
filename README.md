# rustc miscompilation: `str == str` on custom mips64 JSON targets

## Summary

When targeting a custom `mips64-unknown-none` JSON target spec (big-endian MIPS64,
bare-metal), the Rust compiler miscompiles `str` equality comparisons. The generated
assembly is missing the conditional branch on the `memcmp`/`bcmp` return value, so
`str == str` always evaluates to `false`. The same code compiles correctly when using
the built-in `mips64-unknown-linux-gnuabi64` target, even with identical settings in
the JSON spec.

## Prerequisites

A nightly Rust toolchain with `rust-src`:

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

## Reproducing

Build and emit assembly for the custom JSON target:

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

For comparison, build for the built-in Linux target:

```bash
RUSTFLAGS="-Clink-arg=-e_start -Cllvm-args=-mno-check-zero-division" \
  cargo +nightly rustc -Zbuild-std=core \
    --target mips64-unknown-linux-gnuabi64 \
    --release -- --emit=asm
```

```bash
sed -n '/_start:/,/\.end _start/p' target/mips64-unknown-linux-gnuabi64/release/deps/mips_repro-*.s
```

## What to look for

The generated `_start` should contain:

1. A call to `memcmp` or `bcmp`
2. A **conditional branch** on the return value (e.g., `beqz $2, ...`)
3. Two exit paths: `exit(0)` for equal, `exit(1)` for not-equal

### Broken output (custom JSON target: `mips64-unknown-none`)

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

The `beqz` branch on `memcmp`'s return value is **missing**. The `exit(0)` path does
not exist. Both the length-mismatch and length-match paths converge to `exit(1)`.

### Correct output (built-in target: `mips64-unknown-linux-gnuabi64`)

```asm
_start:
    ...
    bne     $6, $1, .LBB1_2     # if lengths differ, jump to exit(1)
    nop
    jalr    $25                  # call bcmp (lengths matched)
    ld      $5, 0($sp)
    beqz    $2, .LBB1_3         # if bcmp == 0 (equal), jump to exit(0)  <-- THIS IS MISSING ABOVE
    nop
.LBB1_2:
    addiu   $1, $zero, 1        # exit(1) — strings not equal
    ...syscall...
.LBB1_3:
    addiu   $1, $zero, 0        # exit(0) — strings equal
    ...syscall...
```

## Source code

```rust
#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let a: &str = core::hint::black_box("name");
    let b: &str = core::hint::black_box("name");
    if a == b {
        exit(0) // expected path
    } else {
        exit(1) // bug: this is the only path emitted for the custom target
    }
}

fn exit(code: u32) -> ! {
    unsafe {
        core::arch::asm!(
            "li $v0, 5058",
            "move $a0, {0}",
            "syscall",
            in(reg) code,
            options(noreturn),
        );
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { exit(2) }
```

`core::hint::black_box` prevents the compiler from constant-folding the comparison,
forcing it to emit the actual comparison logic.

## Affected versions

Tested and confirmed broken on every nightly tried:

- `nightly-2025-07-01` (rustc 1.90.0-nightly)
- `nightly-2025-08-01` (rustc 1.90.0-nightly)
- `nightly-2026-01-15`
- `nightly-2026-02-20`
- `nightly-2026-03-22` (rustc 1.96.0-nightly)

**Note:** Older nightlies (pre-2026) require the `mips64-unknown-none-old.json`
target spec variant, which uses `"target-pointer-width": "64"` (string instead of
integer) and omits the `"abi"` field. The `-Zjson-target-spec` flag is also not
needed on older nightlies.

## Key observations

1. **Built-in vs custom JSON target is the deciding factor.** A JSON target spec with
   settings identical to the built-in `mips64-unknown-linux-gnuabi64` (same cpu,
   features, data-layout, ABI, relocation-model) still produces broken codegen. The
   bug is triggered by using a custom JSON target spec at all, not by any specific
   setting within it.

2. **`[u8]` slice comparison is NOT affected.** Only `str` equality (`==`, `!=`,
   `match` on `&str`) is miscompiled. Byte-slice comparisons work correctly on both
   targets.

3. **Changing individual target spec fields does not fix it.** Tested variations of
   cpu, features, relocation-model, os, and other fields. None resolved the issue.

## Workaround

Use the built-in `mips64-unknown-linux-gnuabi64` target with linker overrides instead
of a custom JSON target spec:

```bash
RUSTFLAGS="-Clink-arg=-e_start -Cllvm-args=-mno-check-zero-division" \
  cargo +nightly rustc -Zbuild-std=core \
    --target mips64-unknown-linux-gnuabi64 \
    --release -- --emit=asm
```

This produces correct assembly with the `beqz` branch present.

## Impact

All `str` comparisons (`==`, `!=`, `match` on `&str`) produce wrong results on
custom mips64 JSON targets. This breaks serde deserialization (field name matching),
string lookups, and any code that compares strings. `[u8]` slice comparisons are not
affected — only `str` equality.
