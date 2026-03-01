# SIMD Optimizations

Using vector instructions for faster matching.

## SIMD Overview

SIMD (Single Instruction, Multiple Data) allows processing multiple bytes simultaneously.

| SIMD Width | Bytes/Iteration | Architecture |
|------------|-----------------|--------------|
| 128-bit    | 16 bytes        | SSE4.1       |
| 256-bit    | 32 bytes        | AVX2         |
| 512-bit    | 64 bytes        | AVX512       |

## Bitap SIMD

The Bitap algorithm is highly parallelizable with SIMD:

```rust
// Without SIMD: 1 byte per iteration
// With AVX2: 32 bytes per iteration
// ~32x speedup potential
```

### Implementation

```rust
// AVX2 implementation
unsafe {
    // Load 32 bytes
    let data = _mm256_loadu_si256(ptr.as_ptr());
    
    // Apply bitap operations to all 32 simultaneously
    // ...
}
```

## Performance Impact

| Configuration | Throughput |
|---------------|------------|
| Scalar (no SIMD) | ~50 MB/s |
| SIMD (AVX2) | ~180 MB/s |
| Speedup | ~3.6x |

## Enabling SIMD

SIMD is enabled by default. To disable:

```toml
[dependencies]
fuzzy-regex = { version = "0.1", default-features = false }
```

## Platform Support

### x86_64

- **SSE4.1**: Minimum for SIMD path
- **AVX2**: Default, 32-byte vectors
- **AVX512**: Optional, 64-byte vectors

### ARM

- **NEON**: 16-byte vectors

### Detection

Runtime detection automatically uses best available:

```rust
// At compile time: #[target_feature(enable = "avx2")]
// At runtime: CPU feature detection
```

## Requirements

SIMD requires:
1. CPU support (SSE4.1+ or NEON)
2. Compiler with SIMD intrinsics
3. Pattern length suitable for Bitap
