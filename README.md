# graemi

A small, fast decompressor for firmware that needs to repeatedly decode small
portions of a larger compressed blob — constant memory, no heap, `#![no_std]`.

Graemi is the fast/simple sibling of [blaris](https://github.com/lis05/blaris):
blaris chases the best possible compression ratio at the cost of decode speed;
graemi gives up some ratio for much cheaper, more predictable decoding. Both
share the same goal — random access into compressed data on constrained
devices — but sit at opposite ends of the ratio/speed trade-off.

## How it works

Graemi is a small-scale take on the ideas behind [density](https://github.com/centaurean/density)
and [firetrail](https://github.com/algorithmnursery/firetrail): a fixed
dictionary of common short words, referenced by a single byte instead of a
general LZ77 match.

- A dictionary of **up to 256 four-byte words** is built from the input.
- Input is tokenized against the dictionary using DP, so tokens are optimal
  *for the given dictionary* (literal byte vs. 4-byte dictionary match).
- Encoding produces two streams:
  - **controls**: 1 bit per token (`0` = literal, `1` = dictionary match), bit-packed
  - **values**: 1 byte per token (a literal byte, or a dictionary index)
- A separate **checkpoint stream** records, at regular intervals, the bit
  offset into the controls stream and the corresponding byte offset in the
  original data.

To decode an arbitrary slice `[offset, offset+len)` of the original data, the
decoder binary-searches the checkpoints for the closest one at or before
`offset`, then decodes forward from there — no need to scan from the start of
the file.

**Complexity:** for an original file of `N` bytes with `K` checkpoints,
decoding a segment of length `L` starting at any offset costs
**O(log K + N/K + L)** time and **O(1)** memory (independent of `N`).

### Compression

Dictionary construction is greedy: repeatedly pick the most frequent
non-overlapping 4-byte word not yet covered by the dictionary, add it, and
mark its occurrences as used. A faster mode is also available that simply
counts word frequencies without tracking overlaps, trading dictionary quality
for build speed.

Checkpoints are placed after tokenization. More checkpoints → larger output
but faster random-access decode. **~0.25% of the original file size** is a
reasonable default checkpoint count.

## On-disk format

```
 4 bytes   B  — bytes per checkpoint (4 or 8)
 4 bytes   N  — input data size
 4 bytes   C  — number of controls (tokens)
 4 bytes   K  — number of checkpoints
 4 bytes   D  — dictionary entries (0..256)
 D*4 bytes    — dictionary (D words, 4 bytes each)
 K*B bytes    — checkpoints (control-bit offset, byte offset), pairs
 ceil(C/8) B  — controls, bit-packed, padded
 C bytes      — values (literal bytes or dictionary indices)
```

Checkpoints use 16-bit fields (`B = 4` total) when both the input and the
control count fit in `u16`, otherwise 32-bit fields (`B = 8`).

## Crate layout

```
graemi/
├─ crates/
│  ├─ graemi-core/        shared types (Word, DICT_LEN, WORD_LEN)
│  ├─ graemi-compress/    encoder
│  ├─ graemi-decompress/  #![no_std] decoder, safe to run on-device
│  └─ graemi-ffi/         C-compatible bindings
└─ src/                   CLI
```

## Usage

### Rust

```rust
// Compress
let bound = graemi_compress::compress_bound(input.len(), checkpoints);
let mut out = vec![0u8; bound];
let written = graemi_compress::compress(&input, &mut out, checkpoints);
out.truncate(written);

// Decompress a slice starting at `offset`, filling `buf`
let mut buf = [0u8; 64];
let ok = graemi_decompress::decompress(&compressed, &mut buf, offset);
```

### C / embedded

`graemi-decompress` is `#![no_std]` and exposes a C ABI entry point:

```c
bool graemi_decompress(
    const uint8_t *from_ptr, size_t from_len,
    uint8_t *to_ptr, size_t to_len,
    size_t offset
);
```

`to_len` bytes of the original data, starting at `offset`, are written into
`to_ptr`. No allocation, no dependency on libc.

## License

MIT
