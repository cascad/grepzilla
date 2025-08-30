pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

pub fn crc64_ecma(data: &[u8]) -> u64 {
    use crc64fast::Digest;
    let mut d = Digest::new();
    d.write(data);
    d.sum64()
}
