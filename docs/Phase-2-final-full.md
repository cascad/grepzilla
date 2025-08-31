# Grepzilla Roadmap — Phase 2 финал (Hardening E → Distribution)

---

# Hardening EPIC E

## [E-H1] — WAL durability, rotation, checksum & fsync
... (как выше) ...

## [E-H7] — Набор метрик, логи, трассировка
... (как выше) ...

---

# EPIC G — Distribution (Shard/Replication/Consistency)

## [G-1] — ManifestStore на etcd

### Контекст
Спрятать etcd за существующим `ManifestStore` trait.

### Цель
Манифесты читаются/пишутся через etcd с CAS.

### Изменения в коде
- `crates/grepzilla_segment/src/manifest_store_etcd.rs` (новый):
  - `EtcdManifestStore` (get_ptr/cas_ptr).
- `crates/broker/http_api/manifest.rs`:
  - внедрить реализацию etcd через DI.

### Acceptance Criteria
- [ ] CAS блокирует гонки публикации между узлами.
- [ ] При недоступном etcd — корректные ретраи и 5xx.

### Тест-план
- [ ] интеграция с локальным etcd; CAS-конкуренция двух клиентов.

---

## [G-2] — Шард-ринг и keyspace

### Контекст
Нужен стабильный способ маршрутизации.

### Цель
Фиксированное N шардов, consistent hash, `replication_factor = R`.

### Изменения в коде
- `crates/broker/cluster/ring.rs` (новый):
  - модель шардов, реплик, версии ринга; сериализация в etcd (`/gz/v1/ring`).

### Acceptance Criteria
- [ ] Детерминированное назначение shard_id по ключу/документу.
- [ ] Обновление ринга атомарно (версионность).

### Тест-план
- [ ] unit: стабильность хеширования; e2e: смена ринга без потери запросов.

---

## [G-3] — Registry узлов и heartbeat (leases)

### Контекст
Отслеживание живости.

### Цель
Узлы регистрируются в `/gz/v1/nodes/<node_id>` с lease TTL.

### Изменения в коде
- `crates/broker/cluster/registry.rs` (новый): регистрация, продление, авто-выпил по TTL.

### Acceptance Criteria
- [ ] Падение узла → запись исчезает ≤ TTL.
- [ ] Метка узла: адреса RPC/HTTP, шард-ownership.

### Тест-план
- [ ] kill узла → перераспределение лидеров (см. G-4).

---

## [G-4] — Лидер-элекшн per-shard

### Контекст
Нужен «primary» на шард.

### Цель
Для каждого shard_id — эксклюзивный лидер (mutex/lease).

### Изменения в коде
- `crates/broker/cluster/leadership.rs` (новый):
  - etcd lock `/gz/v1/shards/<id>/leader`, автопереизбрание.

### Acceptance Criteria
- [ ] В каждый момент времени ровно один лидер на шард.
- [ ] Фейловер за T ≤ TTL + дельта.

### Тест-план
- [ ] конкуренция двух узлов; принудительный разрыв сети.

---

## [G-5] — WAL-репликация (v1: async shipper)

### Контекст
Быстрый первый шаг: «primary» пишет WAL и асинхронно шиппит на реплики.

### Цель
Replica лаг мониторится; чтения можно отправлять на реплики (с eventual).

### Изменения в коде
- `crates/ingest/wal_shipper.rs` (новый):
  - протокол: offset-based pull/push, auth, ретраи.
- `crates/broker/http_api/ingest.rs`:
  - write → primary; 202 при репликации (eventual).

### Acceptance Criteria
- [ ] Новые записи появляются на репликах; лаг метрики доступны.
- [ ] Падение primary не теряет записи (есть WAL на диске).

### Тест-план
- [ ] отключить сеть между primary/replica → догон после восстановления.

---

## [G-6] — Ingest-routing и клиентский UX

### Контекст
Клиент может попасть на любой узел.

### Цель
Любой узел принимает POST /ingest, роутит на правильный «primary».

### Изменения в коде
- `crates/broker/http_api/ingest.rs`:
  - определение shard_id, proxy на leader (или 307 Redirect с location).
  - таймауты/ретраи.

### Acceptance Criteria
- [ ] Пользователь шлёт в любой узел без знания топологии.
- [ ] 404/409 сценарии корректны при смене лидера.

### Тест-план
- [ ] e2e: серия запросов при перетасовке лидеров.

---

## [G-7] — Read path: pin-gen + scatter-gather по шардам

### Контекст
Уже есть SearchCoordinator — расширяем до мультишард.

### Цель
Запрос делится по shard_id, курсор хранит позицию per-shard.

### Изменения в коде
- `crates/broker/search/mod.rs`:
  - shard-aware планировщик; budgets per-shard; dedup.
- `crates/grepzilla_segment/src/cursor.rs`:
  - расширить `SearchCursor` на список `ShardPos`.

### Acceptance Criteria
- [ ] Две страницы подряд стабильно (pin gen per-shard).
- [ ] Частичная деградация при падении части шардов (частичные результаты с флагом).

### Тест-план
- [ ] e2e: многосегментные и мультишардовые кейсы, падение одного шарда.

---

## [G-8] — Consistency model & read-your-writes

### Контекст
Определить и реализовать минимальные гарантии.

### Цель
Документировать eventual/monotonic; опционально `read_your_writes=true` (route to primary).

### Изменения в коде
- `docs/rfcs/0003-consistency.md` (новый): SLA, флаги запроса.
- `crates/broker/http_api/search.rs`:
  - флаг `require_primary=true` → чтение только с лидера шарда.

### Acceptance Criteria
- [ ] При `require_primary=true` пользователь видит свои свежие записи.
- [ ] По умолчанию — eventual; метки «stale_read=false|true».

### Тест-план
- [ ] e2e: POST → немедленный GET с флагом/без флага.

---

## [G-9] — Rebalancer, backfill и anti-entropy

### Контекст
Добавление/удаление узлов, восстановление реплик.

### Цель
Автоматическая догонка: сегменты + WAL offsets.

### Изменения в коде
- `crates/broker/cluster/rebalance.rs`:
  - план копий по новому ring; перенос сегментов; catch-up от нужного WAL offset.
- `crates/ingest/wal_shipper.rs`:
  - запрос диапазонов/снэпшот сегментов.

### Acceptance Criteria
- [ ] После изменения ринга через время T все реплики консистентны.
- [ ] Прогресс виден в метриках.

### Тест-план
- [ ] «переезд» части шардов; измерение времени и отсутствие потерь.

---

## [G-10] — Наблюдаемость кластера и chaos

### Контекст
Нужно видеть здоровье и устойчивость.

### Цель
Метрики кластера и регулярные fault-инъекции.

### Изменения в коде
- `/metrics`: `shard_leader`, `replica_lag`, `rebalance_progress`, `etcd_errors_total`.
- chaos-тесты: kill/partition/slowdisk сценарии в CI job.

### Acceptance Criteria
- [ ] Дэшборд состояния шардов/реплик.
- [ ] Chaos job зелёный при базовых сценариях.

### Тест-план
- [ ] Набор сценариев fault-инъекций (скрипты + отчёт).
