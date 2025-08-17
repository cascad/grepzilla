use crate::index::gram::trigrams;
use croaring::Bitmap;
use std::collections::HashMap;

pub type DocId = u32;

pub struct InvertedIndex {
    next_id: DocId,
    ext2int: HashMap<String, DocId>,
    int2ext: Vec<String>,
    grams: HashMap<String, Bitmap>, // gram -> docs
}

impl InvertedIndex {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            ext2int: HashMap::new(),
            int2ext: Vec::new(),
            grams: HashMap::new(),
        }
    }

    pub fn map_ext_id(&mut self, ext: &str) -> DocId {
        if let Some(&id) = self.ext2int.get(ext) {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.ext2int.insert(ext.to_string(), id);
        self.int2ext.push(ext.to_string());
        id
    }

    pub fn reverse_id(&self, id: DocId) -> &str {
        &self.int2ext[id as usize]
    }

    pub fn add_text(&mut self, doc: DocId, text_norm: &str) {
        for g in trigrams(text_norm) {
            self.grams.entry(g).or_insert_with(Bitmap::new).add(doc);
        }
    }

    pub fn execute(&self, plan: &crate::query::ExecPlan) -> anyhow::Result<Bitmap> {
        use crate::query::BooleanOp::*;
        match plan.op {
            And => self.intersect_all(&plan.grams),
            Or => self.union_all(&plan.grams),
            Not => self.not_all(&plan.grams),
        }
    }

    fn intersect_all(&self, grams: &[String]) -> anyhow::Result<Bitmap> {
        let mut it = grams.iter();
        let first = it.next().ok_or_else(|| anyhow::anyhow!("no grams"))?;
        let mut acc = self.grams.get(first).cloned().unwrap_or_else(Bitmap::new);
        for g in it {
            if let Some(bm) = self.grams.get(g) {
                acc.and_inplace(bm);
            } else {
                acc.clear();
                break;
            }
        }
        Ok(acc)
    }

    fn union_all(&self, grams: &[String]) -> anyhow::Result<Bitmap> {
        let mut acc = Bitmap::new();
        for g in grams {
            if let Some(bm) = self.grams.get(g) {
                acc.or_inplace(bm);
            }
        }
        Ok(acc)
    }

    fn not_all(&self, grams: &[String]) -> anyhow::Result<Bitmap> {
        // Вселенная: [0, next_id)
        let mut acc = Bitmap::new();
        if self.next_id > 0 {
            acc.add_range(0..self.next_id); // croaring 2.x
        }
        for g in grams {
            if let Some(bm) = self.grams.get(g) {
                acc.andnot_inplace(bm);
            }
        }
        Ok(acc)
    }
}

fn range_to_vec(start: u32, end: u32) -> Vec<u32> {
    (start..end).collect()
}
