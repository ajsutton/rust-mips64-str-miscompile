# rustc miscompilation: `str == str` on custom mips64 target with `target-c-int-width: 64`

## Summary

When targeting a custom `mips64-unknown-none` JSON target spec with
`"target-c-int-width": 64`, the Rust compiler miscompiles `str` equality.
The generated assembly is missing the conditional branch on the `memcmp`
return value, so `str == str` always evaluates to `false`.

**Root cause:** `target-c-int-width: 64` tells LLVM that C `int` is 64-bit.
`memcmp` returns `c_int`. LLVM's optimization of the `memcmp` call models
the return value as 64-bit, which causes the branch to be incorrectly
eliminated. Changing to `"target-c-int-width": 32` (the standard for MIPS64
— even on 64-bit MIPS, C `int` is 32 bits per the n64 ABI) fixes the issue.

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

## What to look for

The generated `_start` should contain:

1. A call to `memcmp` or `bcmp`
2. A **conditional branch** on the return value (e.g., `beqz $2, ...`)
3. Two exit paths: `exit(0)` for equal, `exit(1)` for not-equal

### Broken output (`target-c-int-width: 64`)

```asm
_start:
    ...
    bne     $6, $1, .LBB1_2     # if lengths differ, jump to LBB1_2
    nop
    jal     memcmp               # call memcmp (lengths matched)
    ld      $5, 8($sp)
.LBB1_2:
    addiu   $1, $zero, 1         # exit code = 1 (no branch on memcmp result!)
    addiu   $2, $zero, 5058
    move    $4, $1
    syscall                      # exit(1) unconditionally
```

The `beqz` branch on `memcmp`'s return value is **missing**. The `exit(0)` path
does not exist. Both the length-mismatch and length-match paths converge to `exit(1)`.

### Fixed output (`target-c-int-width: 32`)

Changing `"target-c-int-width": 32` in the target spec produces correct code:

```asm
_start:
    ...
    bne     $6, $1, .LBB1_2     # if lengths differ, jump to exit(1)
    nop
    jal     memcmp               # call memcmp (lengths matched)
    ld      $5, 8($sp)
    beqz    $2, .LBB1_3         # if memcmp == 0 (equal), jump to exit(0)
    nop
.LBB1_2:
    addiu   $1, $zero, 1        # exit(1) — strings not equal
    ...syscall...
.LBB1_3:
    addiu   $1, $zero, 0        # exit(0) — strings equal
    ...syscall...
```

## To verify the fix

Edit `mips64-unknown-none.json` and change `"target-c-int-width"` from `64` to `32`,
then rebuild and check the assembly. The `beqz` instruction should now be present.

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
        exit(1) // bug: this is the only path emitted with target-c-int-width: 64
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

## Impact

All `str` comparisons (`==`, `!=`, `match` on `&str`) produce wrong results when
`target-c-int-width` is set to `64` on this target. This breaks serde deserialization
(field name matching), string lookups, and any code that compares strings. `[u8]` slice
comparisons are not affected — only `str` equality which goes through `memcmp`.

The `target-c-int-width` setting does NOT affect pointer width or address space — the
binary is still fully 64-bit MIPS with 64-bit pointers. Only the C `int` type size is
affected. On standard MIPS64 n64 ABI, `int` is 32 bits (only `long` and pointers are
64-bit), so `target-c-int-width: 32` is the correct value.
