//! Graemi is a fast, small decompressor designed for firmware where frequest decompression of
//! small portions of the original data is required.
//!
//! It uses constant memory regardless of the input file size, and unlike blaris
//! (https://github.com/lis05/blaris), which produces the best possible compression ratio but is
//! very slow, graemi is designed to be much faster at a cost of worse ratio.
//!
//! Graemi uses the same idea as density and firetrail compressors, just at a much smaller scale. A
//! dictionary is built, containing at most 256 most common 4-byte-long words. Then, input data
//! is encoded using this dictionary using DP, so for the given dictionary the tokens are indeed
//! optimal.
//!
//! Two streams are created: controls (1 bit per value, bit-packed, 0 for literal, 1 for match) and
//! values (8-bit long, either a literal or match index; offset into the dictionary).
//!
//! To enable fast decompression, synchronization checkpoints are added as a separate stream. They
//! consist of 2 values: offset into the controls stream (in bits), and which offset in
//! the original data (in bytes) corresponds to that control.
//!
//! This way, to decompress a small amount of data, there is no need to search from the beginning
//! of the stream to the destination; instead, a binary search on the checkpoints can be done, and
//! then a much smaller portion of the file needs to be read using the obtained offset from the
//! checkpoint.
//!
//! Specifically, if original file contained N bytes, and there are K checkpoints, segment of size
//! L can be decompressed in O(log(K) + N / K + L).
//!
//! ================================================================================
//!
//! Compression is a simple greedy algorithm. Until the dictionary is filled up, the most common
//! word from thom the original data is placed in the dictionary, and all non-overlapping
//! occurences of it in the data are removed.
//!
//! A faster compression mode is available, which simply counts the frequencies of all words
//! regardless of whether they are overlapping.
//!
//! Checkpoints are calculated after the file has been compressed. The number of checkpoints can be
//! customized: more checkpoints will take more space, but will speed up decompression.
//!
//! Recommended number of checkpoints is 0.25% of the original file size.
//!
//! ================================================================================
//!
//! Format:
//! 4 bytes - (B) bytes per checkpoint (4 or 8)
//! 4 bytes - (N) input data size
//! 4 bytes - (C) number of controls
//! 4 bytes - (K) number of checkpoints
//! 4 bytes - (D) dictionary entries (0..256)
//! D * 4 bytes - dictionary
//! K * B bytes - checkpoints
//! ceil(C / 8) bytes - controls (8 per byte), with padding at the end
//! C bytes - values (literals or match indexes)
//!

use std::collections::HashMap;

use graemi_core::{Word, DICT_LEN, WORD_LEN};

pub fn compress_bound(size: usize, checkpoints: usize) -> usize {
    return 20 + DICT_LEN * WORD_LEN + checkpoints * 8 + (size + 7) / 8 + size;
}

pub fn compress(input: &[u8], output: &mut [u8], mut checkpoints: usize) -> usize {
    if checkpoints == 0 {
        checkpoints = 1;
    }
    if checkpoints > input.len() / 2 + 1 {
        checkpoints = input.len() / 2 + 1;
    }
    assert!(input.len() <= (1 << 31));
    assert!(output.len() >= compress_bound(input.len(), checkpoints));

    // Step 1: compute dictionary

    let mut dict = [Word::default(); DICT_LEN];
    let mut next_word: usize = 0;
    let mut used_bytes = vec![false; input.len()];

    let mut word_positions: HashMap<Word, Vec<usize>> = HashMap::new();
    for i in 0..=input.len().saturating_sub(WORD_LEN) {
        let w: Word = input[i..i + WORD_LEN].try_into().unwrap();
        word_positions.entry(w).or_default().push(i);
    }

    let word_available = |i: usize, used_bytes: &Vec<bool>| {
        if i + WORD_LEN > used_bytes.len() {
            return false;
        }
        for ii in 0..WORD_LEN {
            if used_bytes[i + ii] {
                return false;
            }
        }
        true
    };

    while next_word < dict.len() {
        let mut best_word = Word::default();
        let mut best_count: usize = 0;
        let mut best_hits: Vec<usize> = Vec::new();

        for (word, positions) in &word_positions {
            let mut count = 0usize;
            let mut next_allowed = 0usize;
            let mut hits: Vec<usize> = Vec::new();

            for &p in positions {
                if p < next_allowed {
                    continue;
                }
                if !word_available(p, &used_bytes) {
                    continue;
                }
                count += 1;
                hits.push(p);
                next_allowed = p + WORD_LEN;
            }

            if count > best_count {
                best_word = *word;
                best_count = count;
                best_hits = hits;
            }
        }

        if best_count < 2 {
            break;
        }

        dict[next_word] = best_word;
        next_word += 1;

        for p in best_hits {
            for i in p..p + WORD_LEN {
                used_bytes[i] = true;
            }
        }

        word_positions.remove(&best_word);
    }

    // Step 2: calculate DP

    const MAX: usize = usize::MAX;
    let mut dp_cost = vec![MAX; input.len() + 1]; // in bits
    let mut dp_from = vec![MAX; input.len() + 1];

    dp_cost[0] = 0;
    dp_from[0] = 0;

    for i in 0..input.len() {
        assert!(dp_cost[i] < MAX);
        if i + 1 <= input.len() {
            if dp_cost[i + 1] > 9 + dp_cost[i] {
                dp_cost[i + 1] = 9 + dp_cost[i];
                dp_from[i + 1] = i;
            }
        }

        if i + WORD_LEN <= input.len() {
            let word: Word = input[i..i + WORD_LEN].try_into().unwrap();
            let can_apply = {
                let mut res = false;
                for w in &dict[0..next_word] {
                    if word == *w {
                        res = true;
                        break;
                    }
                }
                res
            };

            if can_apply && dp_cost[i + WORD_LEN] > 9 + dp_cost[i] {
                dp_cost[i + WORD_LEN] = 9 + dp_cost[i];
                dp_from[i + WORD_LEN] = i;
            }
        }
    }

    // Step 3: create streams

    let mut controls = Vec::new();
    let mut values = Vec::new();
    let mut i = input.len();

    assert!(dp_cost[i] < MAX);

    while i != 0 {
        if dp_from[i] == i - 1 {
            controls.push(0);
            values.push(input[i - 1]);
            i = i - 1;
        } else {
            assert!(dp_from[i] == i - WORD_LEN);
            let word: Word = input[i - WORD_LEN..i].try_into().unwrap();

            let index = {
                let mut res = dict.len();
                for (pos, w) in dict[0..next_word].iter().enumerate() {
                    if word == *w {
                        res = pos;
                        break;
                    }
                }
                assert!(res < dict.len());
                res
            };

            controls.push(1);
            values.push(index as u8);
            i = i - WORD_LEN;
        }
    }

    controls.reverse();
    values.reverse();
    assert!(controls.len() == values.len());

    // Step 4: calculate checkpoints

    let mut checkpoint_control = Vec::new();
    let mut checkpoint_pos = Vec::new();
    let mut pos: usize = 0;
    for i in 0..controls.len() {
        while pos >= input.len() * checkpoint_control.len() / checkpoints {
            checkpoint_control.push(i);
            checkpoint_pos.push(pos);
        }

        if controls[i] == 0 {
            pos += 1;
        } else {
            pos += WORD_LEN;
        }
    }

    assert!(pos == input.len());
    assert!(checkpoint_control.len() == checkpoints && checkpoint_pos.len() == checkpoints);

    // Step 5: pack

    let bytes_per_checkpoint = if input.len() < (1 << 16) && controls.len() < (1 << 16) {
        4
    } else {
        8
    };

    let mut offset = 0;

    let store4 = |offset: usize, value: u32, output: &mut [u8]| {
        output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        offset + 4
    };
    let store2 = |offset: usize, value: u16, output: &mut [u8]| {
        output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        offset + 2
    };
    offset = store4(offset, bytes_per_checkpoint as u32, output);
    offset = store4(offset, input.len() as u32, output);
    offset = store4(offset, controls.len() as u32, output);
    offset = store4(offset, checkpoints as u32, output);
    offset = store4(offset, next_word as u32, output);

    let dict_bytes = next_word * WORD_LEN;
    output[offset..offset + dict_bytes].copy_from_slice(dict[0..next_word].as_flattened());
    offset += dict_bytes;

    for i in 0..checkpoints {
        if bytes_per_checkpoint == 4 {
            assert!(
                checkpoint_control[i] <= u16::MAX as usize
                    && checkpoint_pos[i] <= u16::MAX as usize
            );
            offset = store2(offset, checkpoint_control[i] as u16, output);
            offset = store2(offset, checkpoint_pos[i] as u16, output);
        } else {
            assert!(
                checkpoint_control[i] <= u32::MAX as usize
                    && checkpoint_pos[i] <= u32::MAX as usize
            );
            offset = store4(offset, checkpoint_control[i] as u32, output);
            offset = store4(offset, checkpoint_pos[i] as u32, output);
        }
    }

    let control_bytes = (controls.len() + 7) / 8;
    output[offset..offset + control_bytes].fill(0);

    for (i, c) in controls.iter().enumerate() {
        let mut b = output[offset + i / 8];
        let shift = i % 8;
        b ^= c << shift;
        output[offset + i / 8] = b;
    }
    offset += control_bytes;

    for v in values {
        output[offset] = v;
        offset += 1;
    }

    offset
}
