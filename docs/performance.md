# Performance

[Overview](../README.md) · [Guide](guide.md) · [Verification](verification.md) · **Performance**

---

`examples/bench.rs` compresses and decompresses a synthetic 1024 by 512 image of half-float
A, B, G and R channels (smooth, with a little noise) and a float Z channel, block by block,
and reports the best of five runs:

```sh
cargo run --release --example bench
```

## Throughput

Measured on one core of an Intel Xeon at 2.1 GHz, built for generic x86-64, in megapixels
per second:

| Compression | Compress | Decompress | Size |
|---|---|---|---|
| DWAA | 11.8 | 26.4 | 2.1% of the raw image |
| DWAB | 14.2 | 26.6 | 1.5% of the raw image |

## Compared with 0.1

Version 0.2 rewrote the crate as idiomatic Rust. Measured alternately with 0.1 on the same
machine:

| Compression | Compress | Decompress |
|---|---|---|
| DWAA | 17% faster | 39% faster |
| DWAB | 15% faster | 34% faster |

Decompression gains most from converting halves to floats with a table built at compile
time, and from testing for NaN only where a result is one; compression from the
perceptual lookup tables becoming constant arrays.
