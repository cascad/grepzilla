# Grepzilla Roadmap — Phase 2 финал (Hardening E → Distribution)

---

# Hardening EPIC E

## [E-H1] — WAL durability, rotation, checksum & fsync

### Контекст
Повысить надёжность ingest: корректная ротация WAL, защита от частично записанных батчей, гарантии fsync.

### Цель
WAL устойчив к крэшам/перезагрузкам, восстанавливается без потерь или с чётким at-least-once.

### Изменения в коде (пути файлов обязательно!)
- `crates/ingest/wal.rs`:
  - добавить CRC32/xxhash для каждого record (batched frame).
  - режимы durability: `fsync=always|batch|disabled` (config).
  - ротация по `max_bytes` и `max_age`; атомарные имена: `<epoch>-<seq>.wal.tmp` → rename.
- `crates/ingest/hot.rs`:
  - метрики «hot_size», «hot_oldest_ts».
- `crates/broker/http_api/ingest.rs`:
  - propagate ошибки WAL/HotMem с кодами 5xx и `manifest_error`.

### Acceptance Criteria
- [ ] Частично записанный файл WAL после крэша не ломает recovery; битый record срезается по checksum.
- [ ] Ротация срабатывает по лимиту; новые записи уходят в новый файл.
- [ ] `fsync=always` гарантирует, что успешно подтверждённый POST переживает крэш.

### Тест-план
- [ ] unit: валидация checksum (хороший/битый record).
- [ ] e2e: «crash-inject» между write и fsync, затем старт — данные не теряются более чем на последний не-fsync-нутый батч.

---

## [E-H2] — Crash recovery из WAL + HotMem rebuild

### Контекст
После рестарта восстановить HotMem и незавершённые flush’и.

### Цель
Deterministic recovery: HotMem и сегменты консистентны, дубликатов нет.

### Изменения в коде
- `crates/ingest/wal.rs`:
  - `scan_recover(dir) -> Vec<Batch>` с отсечением битых хвостов.
- `crates/ingest/hot.rs`:
  - `rebuild_from_batches(batches)` (idempotent).
- `crates/ingest/flusher.rs`:
  - ретрай незавершённых `.tmp` сегментов (доделать/откатить).

### Acceptance Criteria
- [ ] На холодном старте поднимается HotMem из WAL.
- [ ] Не возникает двойного flush’a для одного и того же батча.

### Тест-план
- [ ] e2e: записать N батчей, симулировать крэш, проверить счётчики в HotMem и итоговый manifest.

---

## [E-H3] — Идемпотентность POST /ingest (Idempotency-Key, de-dup)

### Контекст
Повторы запросов (retry) не должны дублировать документы.

### Цель
Повторный POST с тем же ключом не меняет состояние.

### Изменения в коде
- `crates/broker/http_api/ingest.rs`:
  - поддержка заголовка `Idempotency-Key` (или `ingest_id` в теле).
  - 409/200 с `idempotent=true` при повторе.
- `crates/ingest/hot.rs`:
  - в HotMem — set последнего offset по ключу; сквозная дедупликация по `doc_id` (если есть поле).

### Acceptance Criteria
- [ ] Двойной POST с тем же ключом не создаёт дубликаты.
- [ ] Ответ содержит флаг идемпотентности.

### Тест-план
- [ ] e2e: два одинаковых запроса → одинаковый `segment`, `hot_added` не увеличивается.

---

## [E-H4] — Backpressure, 413/429, лимиты и квоты

### Контекст
Защититься от перегруза и больших тел.

### Цель
Ограничения размера/скорости и предсказуемые ошибки.

### Изменения в коде
- `crates/broker/http_api/mod.rs`:
  - лимит тела запроса (конфиг), 413.
  - rate-limit/токен-бакет на `POST /ingest`, 429 с `Retry-After`.
- `crates/ingest/hot.rs`:
  - watermarks: soft/hard cap → 503 с `backlog_ms`.

### Acceptance Criteria
- [ ] Большие запросы получают 413.
- [ ] При переполнении HotMem — 503 и корректные заголовки.

### Тест-план
- [ ] нагрузочный тест: рост RPS до троттлинга, мониторинг кодов/метрик.

---

## [E-H5] — Atomic publish сегмента и GC старых файлов

### Контекст
Исключить «висячие» сегменты и гонки публикации.

### Цель
Публикация сегмента атомарна; старые файлы собираются по манифесту.

### Изменения в коде
- `crates/ingest/flusher.rs`:
  - запись в `.tmp`, fsync, затем rename.
- `crates/ingest/compactor.rs`:
  - двухфазная публикация: (1) записать/синкануть сегмент, (2) обновить манифест, (3) GC по tombstone/obsolete спискам.
- `crates/broker/http_api/manifest.rs`:
  - чёткие статусы ошибок публикации.

### Acceptance Criteria
- [ ] После крэша не остаётся «полусегментов» в манифесте.
- [ ] GC удаляет ровно те файлы, что не упоминаются ни в одном gen.

### Тест-план
- [ ] e2e: искусственно прервать между шагами 1/2 — при старте система чинит состояние.

---

## [E-H6] — Нормализация/валидация входных документов

### Контекст
Стабильное ingest с «грязным» входом.

### Цель
Строгая схема: типы, размер полей, обрезка, defaults.

### Изменения в коде
- `crates/broker/http_api/ingest.rs`:
  - схема: обязательные/опциональные поля, max len, drop/trim.
  - 4xx коды и детальные `errors[]`.

### Acceptance Criteria
- [ ] Неверные документы не проходят; валидные — сохраняются.
- [ ] Ответ содержит список ошибок по документам.

### Тест-план
- [ ] unit: граничные значения; e2e: смешанный батч валидных/невалидных.

---

## [E-H7] — Набор метрик, логи, трассировка

### Контекст
Нужно наблюдение: латентность ingest → hot → flush → publish.

### Цель
Полные метрики и корреляция по trace-id.

### Изменения в коде
- `crates/broker/*` / `crates/ingest/*`:
  - Prometheus: `ingest_requests`, `wal_bytes_total`, `flush_duration_ms`, `hot_mem_size`, `manifest_publish_total{status}` и т.п.
  - лог-корреляция `trace_id` из заголовка/генерации.

### Acceptance Criteria
- [ ] Дэшборд показывает SLA ingest.
- [ ] По trace-id видно путь одного батча.

### Тест-план
- [ ] smoke: scrape `/metrics`, проверка метрик при разных сценариях.

---

# EPIC G — Distribution (Shard/Replication/Consistency)

## [G-1] — ManifestStore на etcd
... (и так далее до G-10)
