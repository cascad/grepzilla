pub const META_MAGIC: u32 = 0x475A5347; // "GZSG"
pub const META_VERSION: u16 = 2;
pub const META_HEADER_LEN: u16 = 48;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MetaHeader {
    pub magic: u32,
    pub version: u16,
    pub header_len: u16,
    pub doc_count: u64,
    pub gram_count: u64,
    pub grams_idx_len: u64,
    pub grams_dat_len: u64,
    pub fields_idx_len: u64,
    pub fields_dat_len: u64,
    pub docs_dat_len: u64,
}

impl Default for MetaHeader {
    fn default() -> Self {
        Self {
            magic: META_MAGIC,
            version: META_VERSION,
            header_len: META_HEADER_LEN,
            doc_count: 0,
            gram_count: 0,
            grams_idx_len: 0,
            grams_dat_len: 0,
            fields_idx_len: 0,
            fields_dat_len: 0,
            docs_dat_len: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredDoc {
    pub ext_id: String,
    /// Отсортирован по field_id
    pub fields: Vec<(u32, String)>,
}