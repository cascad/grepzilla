# RFC-0001: Search & Replication Flows (etcd-manifest, hyperscale-ready)

**Файл:** `docs/rfcs/0001-search-and-replication-flows.md`

**Статус:** Draft → Proposed
**Дата:** 2025-08-20
**Автор:** Grepzilla Team

## Резюме

Задаём последовательности действий (flows) и стабильные интерфейсы для чтения/записи в Grepzilla при eventual consistency. Управление видимостью данных — через неизменяемые **манифесты сегментов** и небольшой консенсус (etcd/Raft) только по **указателю на актуальный манифест**. Позже допускается миграция в leaderless (Dynamo-style) без поломки API.

---

## Цели

* Предсказуемая видимость данных и стабильные курсоры в eventual‑репликации.
* Простые, неизменяемые **сегменты**; обновления/удаления — через новые манифесты и tombstones.
* Стриминг поиска с бюджетами; масштабирование по шард‑группам.
* Возможность эволюции control‑plane: **сейчас etcd**, позже leaderless.

## Не цели

* Полная линейная консистентность данных.
* Физический формат сегмента V2 (mmap/FST/blocks) — вне этого RFC (см. отдельный документ).

---

## Компоненты

* **Client** — внешний потребитель API.
* **Broker** — фронт поиска (парсер, планировщик, merge, курсоры, budgets).
* **Ingestor** — фронт записи (WAL/memtable/flush/compaction).
* **Shard Group** — RF=2–3 узла хранения каждого шарда.
* **Storage Node** — кэш сегментов, префильтр/verify "рядом с данными".
* **Object Storage** — S3/GCS‑совместимое хранилище сегментов/манифестов.
* **etcd** — маленький консенсус по `manifest_ptr` (per shard).

---

## Модель данных

### 1) Указатель на манифест (manifest\_ptr)

**Хранение:** etcd ключ `/grepzilla/shards/<shard_id>/manifest_ptr`

```json
{
  "epoch": 3,
  "gen": 123,
  "url": "s3://grepzilla-segments/shard-17/gen-000123/manifest.json",
  "checksum": "sha256:...",
  "updated_at": "2025-08-20T12:00:00Z"
}
```

* `epoch` — смена конфигурации/назначения шарда.
* `gen` — монотонная генерация "среза" сегментов.
* `url` — объект `manifest.json` в object storage.
* Обновление атомарно через etcd (CAS по версии/lease).

### 2) Иммутабельный манифест `manifest(gen)`

**Хранение:** объект в `s3://.../shard-<id>/gen-<gen>/manifest.json`

```json
{
  "version": 1,
  "shard_id": 17,
  "gen": 123,
  "created_at": "2025-08-20T12:00:00Z",
  "hwm_seqno": "hlc:17:9123456",
  "segments": [
    {"id":"s-abc","url":".../s-abc","min_doc":0,"max_doc":9999,"time_min":1690000000,"time_max":1690099999},
    {"id":"s-def","url":".../s-def","min_doc":10000,"max_doc":19999,"time_min":1690100000,"time_max":1690199999}
  ],
  "tombstones": {"cardinality": 5321, "url": ".../tombstones_123.roaring"},
  "prev_gen": 122
}
```

* Содержит полный состав сегментов и ссылку на карту удалений.
* Не меняется после публикации.

### 3) Версии документов

* Каждому upsert присваивается `version` = **HLC** (`hlc:<shard>:<counter>`).
* В сегментах и tombstones храним `ext_id -> version`.
* При merge результатов берём `max(version)` per `_id`.

### 4) Курсор выдачи

```json
{
  "matcher_hash": "sha256:...",
  "pin_gen": {"17":123,"18":98},
  "state": [
    {"shard":17, "segment":"s-abc", "block": 42, "last_docid": 123456},
    {"shard":18, "segment":"s-xyz", "block": 7,  "last_docid": 9876}
  ],
  "budgets": {"candidates": 100000, "verify_ms": 500}
}
```

* `pin_gen` — закрепление генерации для стабильности пагинации.
* `state` — координаты продолжения внутри сегментов.

---

## Flows (последовательности действий)

### A) Ingest (upsert)

1. Client → Ingestor: `POST /ingest` (batch JSON: `_id`, поля, опционально `_seqno`).
2. Ingestor назначает `version = HLC` и пишет запись в **WAL** (`W=QUORUM`).
3. Обновляет **memtable (L0)** на узле шарда.
4. По триггеру flush строит **малый сегмент**, загружает в Object Storage.
5. Формирует **новый `manifest(gen+1)`**, затем **атомарно** обновляет `manifest_ptr` в etcd.
6. Реплики подтягивают сегмент асинхронно (anti‑entropy/watchers).

**Видимость:** чтения по закреплённому `gen` увидят изменения только после шага 5. До этого свежие данные доступны в L0 узла‑принятия (read path: `L0 → segments(gen)`).

### B) Delete (tombstone)

1. Client → Ingestor: `DELETE /doc/_id`.
2. Ingestor формирует tombstone (`_id`, `version`), пишет в WAL (`W=QUORUM`).
3. На flush добавляет tombstones‑файл, публикует `manifest(gen+1)`, обновляет pointer (etcd).
4. Компакция удаляет старые версии после `tombstone_safe_point` (все RF на `gen≥G`).

### C) Search (первая страница)

1. Broker читает `manifest_ptr` из etcd по целевым шардам.
2. Скачивает `manifest(gen)` из Object Storage, закрепляет `gen` в сессии/курсуле.
3. Парсит запрос, нормализует, извлекает обязательные литералы ≥3 → 3‑граммы.
4. Планирует по `segments` (time/labels), назначает **budgets** per сегмент.
5. Шлёт на Storage Nodes задания **prefilter** (Roaring по n‑gram + field mask).
6. Узлы возвращают кандидатов порциями; узлы/или Broker выполняют **verify** (regex DFA/hyperscan) по DocStore.
7. Broker делает **dedup `_id`** с учётом `version`, стримит hits (`ext_id, score, preview`).
8. Возвращает страницу + **cursor** (`pin_gen`, координаты, budgets, progress\`).

**Consistency:** `ONE|QUORUM|ALL`. Для `QUORUM` Broker мержит результаты с 2/3 реплик; может выполнять read‑repair по `manifest_ptr`.

### D) Search (следующая страница)

1. Client → Broker: `POST /search` с `cursor`.
2. Broker проверяет `matcher_hash` и **pin\_gen**.
3. Продолжает с координат (те же сегменты/реплики по возможности), применяет budgets.
4. Возвращает следующую страницу и обновлённый cursor.

**Failover:** если реплика недоступна — переключение на другую реплику того же `gen`. Иначе `cursor expired` (редко — при отсутствии сегментов нужного `gen`).

### E) Compaction во время запросов

* Компакция создаёт новые сегменты и публикует `manifest(gen+1)`, обновляет pointer в etcd.
* Идущие запросы остаются на своём `gen` до конца сессии; GC удаляет старые сегменты после `safe_point`.

### F) Read‑after‑write

* До публикации `gen` свежак виден в L0 узла, принявшего запись (и возможно на других при репликации WAL).
* Параметр запроса `max_staleness_ms` может заставить Broker дождаться `hwm_seqno`.

### G) Anti‑entropy/Repair

* Наблюдатели сравнивают `manifest_ptr` и локальный кеш; реплика догружает сегменты/тумбстоуны.
* Меркл‑деревья по сегментам/манифестам для сверки на больших объёмах; hinted handoff.

---

## API (эскиз)

### `GET /manifest/:shard?gen=<n>` → `manifest.json`

* Источник: Object Storage. Если `gen` не задан — Broker читает `manifest_ptr` из etcd.

### `POST /search`

```json
{
  "tenant": "default",
  "target": {"shards": [17,18]},
  "consistency": "ONE",
  "pin_gen": {"17":123,"18":98},
  "query": {"wildcard": "*кот*", "field": null, "time": null, "labels": []},
  "page": {"size": 50, "cursor": null},
  "budget_hint": {"candidates": 200000, "verify_ms": 500}
}
```

**Ответ (стриминг):** `hits[]` + `cursor` + `progress` (percent by segments). Для `QUORUM` включать `read_repair` поле.

### `POST /ingest` (batch)

* Идём в Ingestor. Ответ — ack с максимальным `version`/`seqno`.

### `DELETE /doc/:id`

* Создаёт tombstone. Ответ — ack с `version`.

---

## Консистентность и курсоры

* `consistency = ONE|QUORUM|ALL` — только для чтения.
* Курсор всегда несёт `pin_gen`. Переход на новый `gen` — только с новой сессии.
* В merge всегда `distinct _id by max(version)`.

---

## Операционные аспекты

* **GC:** удаление superseded сегментов после `tombstone_safe_point` (все RF ≥ gen G).
* **Мониторинг:** p50/p95/p99 (prefilter/verify/total), candidates per segment, budgets hit‑rate, cache hit‑rate, ingest lag, compaction debt.
* **Аудит:** журнал смен pointer’ов (кто/когда), соответствие checksum.

---

## Безопасность/Доступ

* Подписи/ACL на объектных ключах сегментов/манифестов.
* RBAC на API брокера/ингестора.

---

## План миграции к leaderless (Dynamo‑style)

* Сделать манифесты иммутабельными (уже так).
* Заменить etcd‑pointer на запись/чтение `manifest_ptr` с `W/R = QUORUM`, версия — HLC/векторные часы.
* Включить анти‑энтропию для выравнивания pointer’ов.
* Курсоры уже совместимы (несут `gen`).

---

## Вопросы/Открытые темы

* Гранулярность budgets per segment: как адаптировать под Zipf‑распределение грамм?
* Политика TTL для tombstones (7/14/30 дней?)
* Выбор verify‑движка (regex‑automata vs Hyperscan) и правила fallback.
* Версионирование DocStore: храним только нормализованный текст или сырой + нормализованный?

---

## Приложение: Соглашения и типы

* `gen`: 64‑бит, монотонный в рамках `epoch`.
* `version (HLC)`: строка `hlc:<shard>:<counter>` или 128‑бит.
* `doc_id`: 32‑бит локальный в сегменте; `ext_id`: строка/UUID.
* `Bitmap`: Roaring (u32 doc\_id); tombstones — Roaring по хэшу `_id` или по локальным id.
