# Reproducer: `str == str` broken on custom mips64 target with wrong `target-c-int-width`

## Summary

When using a custom `mips64-unknown-none` JSON target spec with
`"target-c-int-width": 64`, `str` equality comparisons are miscompiled.
The generated assembly is missing the conditional branch on the `memcmp`
return value, so `str == str` always evaluates to `false`.

**This is not a compiler bug.** The MIPS64 n64 ABI specifies C `int` as
32 bits. Setting `target-c-int-width: 64` is an incorrect target
configuration that causes LLVM to generate wrong code for `memcmp`-based
comparisons. The fix is to set `"target-c-int-width": 32`.

## Fix

```diff
- "target-c-int-width": 64,
+ "target-c-int-width": 32,
```

## Reproducing

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly

RUSTFLAGS="-Clink-arg=-e_start -Cllvm-args=-mno-check-zero-division" \
  cargo +nightly rustc -Zbuild-std=core -Zjson-target-spec \
    --target ./mips64-unknown-none.json \
    --release -- --emit=asm

sed -n '/_start:/,/\.end _start/p' target/mips64-unknown-none/release/deps/mips_repro-*.s
```

With `target-c-int-width: 64` (broken), the assembly has no `beqz` branch
after `memcmp` — it unconditionally takes the "not equal" path.

With `target-c-int-width: 32` (fixed), the `beqz` branch is present and
both `exit(0)` and `exit(1)` paths exist.

## References

- [MIPSpro 64-Bit Porting Guide](https://math-atlas.sourceforge.net/devel/assembly/mipsabi64.pdf) — "64-bit pointer and long and 32-bit int"
- [MIPSpro N32 ABI Handbook](https://math-atlas.sourceforge.net/devel/assembly/007-2816-005.pdf) — `int` is 32 bits in both n32 and n64 ABIs
