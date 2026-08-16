use graemi_compress::{compress, compress_bound};
use graemi_decompress::decompress;

fn verify_full_and_random_access(data: &[u8], checkpoints: usize) {
    let mut compressed = vec![0u8; compress_bound(data.len(), checkpoints)];
    let compressed_len = compress(data, &mut compressed, checkpoints);
    let compressed_slice = &compressed[..compressed_len];

    // Full decompression
    let mut decompressed = vec![0u8; data.len()];
    let ok = decompress(compressed_slice, &mut decompressed, 0);
    assert!(ok, "Full decompression failed");
    assert_eq!(decompressed, data, "Full decompressed data mismatch");

    if data.is_empty() {
        return;
    }

    // Random-access chunk reads at various offsets and lengths
    let test_windows = [1, 2, 3, 4, 5, 7, 8, 16, 33, 64];
    let step = (data.len() / 20).max(1);

    for offset in (0..data.len()).step_by(step) {
        for &win_len in &test_windows {
            if offset + win_len <= data.len() {
                let mut chunk = vec![0u8; win_len];
                let ok = decompress(compressed_slice, &mut chunk, offset);
                assert!(ok, "Random access decompression failed at offset {offset} (len {win_len})");
                assert_eq!(
                    chunk,
                    &data[offset..offset + win_len],
                    "Random access data mismatch at offset {offset} (len {win_len})"
                );
            }
        }

        // Decompress from offset to the end
        let remaining_len = data.len() - offset;
        let mut tail = vec![0u8; remaining_len];
        let ok = decompress(compressed_slice, &mut tail, offset);
        assert!(ok, "Tail decompression failed at offset {offset}");
        assert_eq!(
            tail,
            &data[offset..],
            "Tail decompressed data mismatch at offset {offset}"
        );
    }
}

fn generate_pattern_data(size: usize) -> Vec<u8> {
    let words = [
        *b"ABCD",
        *b"1234",
        *b"test",
        *b"rust",
        *b"data",
        *b"wrap",
    ];

    let mut data = Vec::with_capacity(size);
    let mut idx = 0;
    while data.len() < size {
        let remaining = size - data.len();
        if remaining >= 4 && (idx % 3 != 0) {
            let word = words[idx % words.len()];
            data.extend_from_slice(&word);
        } else {
            data.push(((idx * 7 + 13) % 256) as u8);
        }
        idx += 1;
    }
    data
}

#[test]
fn test_small_data() {
    let small_data = b"Hello, Graemi! Hello, Graemi! ABCD1234ABCD1234";
    verify_full_and_random_access(small_data, 1);
    verify_full_and_random_access(small_data, 2);
    verify_full_and_random_access(small_data, 5);
}

#[test]
fn test_big_data() {
    let big_data = generate_pattern_data(4096);
    let checkpoints = 40;
    verify_full_and_random_access(&big_data, checkpoints);
}

#[test]
fn test_16kb_data() {
    let data_16k = generate_pattern_data(16 * 1024);
    let checkpoints = 160; // ~1% checkpoints
    verify_full_and_random_access(&data_16k, checkpoints);
}

#[test]
fn test_32kb_data() {
    let data_32k = generate_pattern_data(32 * 1024);
    let checkpoints = 320; // ~1% checkpoints
    verify_full_and_random_access(&data_32k, checkpoints);
}

#[test]
fn test_checkpoints_1_percent() {
    let data = generate_pattern_data(2000);
    let checkpoints = (data.len() * 1 / 100).max(1); // 1% checkpoints = 20
    verify_full_and_random_access(&data, checkpoints);
}

#[test]
fn test_checkpoints_10_percent() {
    let data = generate_pattern_data(1000);
    let checkpoints = (data.len() * 10 / 100).max(1); // 10% checkpoints = 100
    verify_full_and_random_access(&data, checkpoints);
}

#[test]
fn test_random_accesses() {
    let data = generate_pattern_data(1024);
    let checkpoints = 16;
    let mut compressed = vec![0u8; compress_bound(data.len(), checkpoints)];
    let compressed_len = compress(&data, &mut compressed, checkpoints);
    let compressed_slice = &compressed[..compressed_len];

    // Single-byte reads at every position
    for pos in 0..data.len() {
        let mut single_byte = [0u8; 1];
        assert!(decompress(compressed_slice, &mut single_byte, pos));
        assert_eq!(single_byte[0], data[pos], "Mismatch at pos {pos}");
    }

    // Arbitrary slice lengths crossing word / checkpoint boundaries
    for offset in [0, 1, 2, 3, 4, 5, 7, 13, 31, 64, 127, 255, 500, 750] {
        for len in [1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 32, 64] {
            if offset + len <= data.len() {
                let mut buf = vec![0u8; len];
                assert!(decompress(compressed_slice, &mut buf, offset));
                assert_eq!(buf, &data[offset..offset + len]);
            }
        }
    }
}

#[test]
fn test_full_decompression() {
    for size in [16, 32, 64, 128, 256, 512, 1024, 2048] {
        let data = generate_pattern_data(size);
        for checkpoints in [1, 2, 4, 8] {
            if checkpoints <= data.len() / 2 + 1 {
                let mut compressed = vec![0u8; compress_bound(data.len(), checkpoints)];
                let compressed_len = compress(&data, &mut compressed, checkpoints);
                let mut decompressed = vec![0u8; size];
                assert!(decompress(&compressed[..compressed_len], &mut decompressed, 0));
                assert_eq!(decompressed, data, "Failed for size {size} with {checkpoints} checkpoints");
            }
        }
    }
}
