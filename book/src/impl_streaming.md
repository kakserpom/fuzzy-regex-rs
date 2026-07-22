# Streaming Implementation

Processing data incrementally without loading everything into memory.

## Streaming Architecture

```text
Input Data
    │
    ▼
┌──────────────────┐
│  StreamingMatcher │
│  ┌──────────────┐ │
│  │ Input Buffer │ │
│  └──────────────┘ │
│         │         │
│         ▼         │
│  ┌──────────────┐ │
│  │   Matcher    │ │
│  └──────────────┘ │
└────────┬─────────┘
         │
         ▼
    Match Results
```

## Key Components

### Input Buffer

Stores unprocessed data between feed calls:

```rust,ignore
struct StreamingMatcher {
    buffer: Vec<u8>,      // Unprocessed bytes
    position: usize,      // Current position in stream
    matcher: Matcher,     // Underlying matcher
}
```

### Position Tracking

Tracks absolute position in the stream:

```rust,ignore
let mut stream = re.stream();

stream.feed(b"hello");      // position = 5
stream.feed(b" world");     // position = 11

assert_eq!(stream.position(), 11);
```

## Cross-Boundary Matching

Matches can span chunk boundaries:

```rust,ignore
let mut stream = re.stream();

// Chunk 1: no complete match
stream.feed(b"xxx ");  

// Chunk 2: completes the match
let matches = stream.feed(b"hello").collect();
// Match found spanning from chunk 1
```

### Buffer Management

```rust,ignore
// Keep some overlap between chunks
// for cross-boundary matches

let mut stream = re.stream();

// Strategy: Keep last (pattern_len + max_errors) bytes
// This ensures cross-boundary matches are found
```

## Reader Integration

Process any `Read` source:

```rust,ignore
use std::io::BufReader;
use std::fs::File;

let file = File::open("large_file.txt")?;
let reader = BufReader::new(file);

for m in re.stream().with_chunk_size(8192).search_reader(reader) {
    println!("Match at {}", m.start());
}
```

## Byte-Level Streaming

### Feed

```rust,ignore
stream.feed(b"data chunk");
```

### Finish

```rust,ignore
// After all data fed, check for remaining matches
stream.finish();
```

### Reset

```rust,ignore
// Start over with fresh state
stream.reset();
```

## Performance Considerations Size

### Chunk

| Chunk Size    | Use Case                 |
|---------------|--------------------------|
| Small (1KB)   | Low latency, interactive |
| Medium (8KB)  | Balanced                 |
| Large (64KB+) | High throughput          |

### Memory Usage

Streaming uses O(max_pattern_length + buffer) memory regardless of input size.

## Implementation Details

### State Management

```rust,ignore
// Between feed calls:
// 1. Save current state
// 2. Process new chunk
// 3. Keep partial state for next call

struct State {
    nfa_state: Option<NfaState>,
    bitap_state: u64,
    // etc.
}
```

### Partial Results

Report matches as they're found:

```rust
// Yield matches immediately as they're found
// Don't wait for complete input
```
