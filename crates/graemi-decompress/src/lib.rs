#![no_std]

pub use graemi_core::*;

pub fn decompress(input: &[u8], output: &mut [u8], offset: usize) -> bool {
    if input.len() < 20 {
        return false;
    }

    let load4 = |pos| u32::from_le_bytes(input[pos..pos + 4].try_into().unwrap());

    let b = load4(0) as usize;
    //let n = load4(4) as usize;
    let c = load4(8) as usize;
    let k = load4(12) as usize;
    let d = load4(16) as usize;

    if input.len() < 20 + d * WORD_LEN + k * b + (c + 7) / 8 + c {
        return false;
    }

    let rem = &input[20..];
    let (dictionary, rem) = rem.split_at(d * WORD_LEN);
    let (checkpoints, rem) = rem.split_at(b * k);
    let (controls, rem) = rem.split_at((c + 7) / 8);
    let values = &rem[..c];

    let mut checkpoint = 0;
    let mut c_control = 0;
    let mut c_offset = 0;
    for pow2 in (0..k.ilog2()).rev() {
        let new_checkpoint = checkpoint + (1 << pow2);
        if new_checkpoint < k {
            let new_control: usize;
            let new_offset: usize;

            if b == 4 {
                new_control =
                    u16::from_le_bytes(checkpoints[4 * new_checkpoint..][..2].try_into().unwrap())
                        as usize;
                new_offset = u16::from_le_bytes(
                    checkpoints[4 * new_checkpoint + 2..][..2]
                        .try_into()
                        .unwrap(),
                ) as usize;
            } else {
                new_control =
                    u32::from_le_bytes(checkpoints[8 * new_checkpoint..][..4].try_into().unwrap())
                        as usize;
                new_offset = u32::from_le_bytes(
                    checkpoints[8 * new_checkpoint + 4..][..4]
                        .try_into()
                        .unwrap(),
                ) as usize;
            }

            if new_offset <= offset {
                checkpoint = new_checkpoint;
                c_control = new_control;
                c_offset = new_offset;
            }
        }
    }

    if c_offset > offset {
        c_control = 0;
        c_offset = 0;
    }

    let mut bytes_copied = 0;
    while bytes_copied < output.len() {
        let control_byte = controls[c_control / 8];
        let control = (control_byte >> (c_control % 8)) & 1;
        if control == 0 {
            if offset <= c_offset {
                output[bytes_copied] = values[c_control];
                bytes_copied += 1;
            }
            c_offset += 1;
        } else {
            if !(c_offset + WORD_LEN <= offset) {
                for i in 0..WORD_LEN {
                    if c_offset + i >= offset && bytes_copied < output.len() {
                        output[bytes_copied] =
                            dictionary[WORD_LEN * values[c_control] as usize + i];
                        bytes_copied += 1;
                    }
                }
            }
            c_offset += WORD_LEN;
        }
        c_control += 1;
    }

    true
}

/// C-compatible entry point for decompression.
///
/// # Safety:
/// `from_ptr` and `to_ptr` must be non-null and valid for reads/writes up to `from_len` and `to_len` respectively.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn graemi_decompress(
    from_ptr: *const u8,
    from_len: usize,
    to_ptr: *mut u8,
    to_len: usize,
    offset: usize,
) -> bool {
    if from_ptr.is_null() || to_ptr.is_null() {
        return false;
    }

    // SAFETY: from_ptr and from_len form a valid slice due to the safety contract.
    let from = unsafe { core::slice::from_raw_parts(from_ptr, from_len) };
    // SAFETY: to_ptr and to_len form a valid slice due to the safety contract.
    let to = unsafe { core::slice::from_raw_parts_mut(to_ptr, to_len) };

    decompress(from, to, offset)
}
