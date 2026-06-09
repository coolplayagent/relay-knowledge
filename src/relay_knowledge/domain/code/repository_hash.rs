pub(super) fn append_hash_list(input: &mut Vec<u8>, values: &[String]) {
    input.extend_from_slice(&(values.len() as u64).to_le_bytes());
    for value in values {
        append_hash_part(input, value);
    }
}

pub(super) fn append_hash_part(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&(value.len() as u64).to_le_bytes());
    input.extend_from_slice(value.as_bytes());
}

pub(super) fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}
