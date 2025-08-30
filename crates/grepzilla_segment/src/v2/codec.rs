use std::io::Write;

pub fn put_varint(mut x: u64, out: &mut Vec<u8>) {
    while x >= 0x80 {
        out.push(((x as u8) & 0x7F) | 0x80);
        x >>= 7;
    }
    out.push(x as u8);
}

pub fn get_varint(mut bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut shift = 0u32;
    let mut val = 0u64;
    for (i, b) in bytes.iter().enumerate() {
        let byte = *b as u64;
        val |= (byte & 0x7F) << shift;
        if (byte & 0x80) == 0 {
            return Some((val, &bytes[i + 1..]));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

/// Записать varint прямо в `std::io::Write`.
pub fn put_varint_to_writer<W: Write>(mut x: u64, w: &mut W) -> std::io::Result<()> {
    while x >= 0x80 {
        w.write_all(&[((x as u8) & 0x7F) | 0x80])?;
        x >>= 7;
    }
    w.write_all(&[x as u8])?;
    Ok(())
}
