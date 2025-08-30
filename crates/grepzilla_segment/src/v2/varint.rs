// crates/grepzilla_segment/src/v2/varint.rs
use std::io::{self, Read, Write};

pub fn write_uvarint<W: Write>(mut w: W, mut x: u64) -> io::Result<()> {
    while x >= 0x80 {
        w.write_all(&[((x as u8) | 0x80)])?;
        x >>= 7;
    }
    w.write_all(&[x as u8])?;
    Ok(())
}

pub fn read_uvarint<R: Read>(mut r: R) -> io::Result<u64> {
    let mut x: u64 = 0;
    let mut s: u32 = 0;
    loop {
        let mut buf = [0u8;1];
        if r.read(&mut buf)? != 1 { return Err(io::ErrorKind::UnexpectedEof.into()); }
        let b = buf[0];
        if b < 0x80 {
            if s >= 64 { return Err(io::ErrorKind::InvalidData.into()); }
            return Ok(x | ((b as u64) << s));
        }
        x |= ((b & 0x7F) as u64) << s;
        s += 7;
        if s >= 64 { return Err(io::ErrorKind::InvalidData.into()); }
    }
}
