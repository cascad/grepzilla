// storage/src/segment/reader.rs (фрагмент - вставь в конец файла)
pub struct SegIter<'a> {
    // mmap или reader + буферизация
    // current offset -> next record
    phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> Iterator for SegIter<'a> {
    type Item = anyhow::Result<RecordView>; // лёгкая обёртка над &str/&[u8] без аллокаций

    fn next(&mut self) -> Option<Self::Item> {
        // читать следующую строку/запись
        // уважать лимиты, проверять отмену (если через внешние флаги)
        todo!()
    }
}
