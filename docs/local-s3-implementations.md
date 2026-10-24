# Local S3: s3s-fs или Garage

Проверено по официальным исходникам и документации релизов `s3s v0.17.0` и
`Garage v2.4.1` (24 октября 2026). Для Catalog обязательны SigV4, path-style URL,
presigned `PUT` с подписанными `content-type` и `x-amz-checksum-sha256`,
`HeadObject` с checksum mode и идемпотентный `DeleteObject`.

| Критерий | s3s-fs | Garage |
|---|---|---|
| Назначение и зрелость | `s3s` называет проект experimental, а `s3s-fs` — sample implementation, специально предназначенной для integration testing и mock S3 client. Серия `0.x`; `v0.17.0` выпущен 2026-09-24, репозиторий активен. | Полноценное distributed object storage; используется Deuxfleurs в production с первого релиза 2020 года. Серия `2.x`; `v2.4.1` выпущен 2026-09-07, репозиторий активен. |
| Запуск локально | Один scratch-контейнер из GHCR: `s3s-fs --host 0.0.0.0 --port 8014 --access-key … --secret-key … /data`. Для Gavel достаточно пробросить один порт; bucket создаёт `ensure_bucket`. | Нужны `garage.toml`, RPC/admin secrets, `metadata_dir`, `data_dir`, region и несколько listeners. Упрощённый путь — `garage server --single-node --default-bucket` плюс env для key/secret/bucket; есть Docker image и Helm single-node mode. |
| S3 API | Best-effort practical compatibility, не заявляет 100%. В `v0.17.0` файловый backend реализует требуемые `PutObject`, `HeadObject`, `DeleteObject`, проверяет/сохраняет SHA-256 checksum; проект гоняет AWS SDK, boto3, rclone, Ceph s3-tests и Mint flows. | Документированная матрица: SigV4, path/vhost style, presigned URL и все требуемые core endpoints. В `v2.4.1` исходники принимают/проверяют/возвращают `x-amz-checksum-sha256`. Нет versioning, object lock, S3 replication и ряда AWS-specific API — Catalog они не нужны. |
| Persistence | Объекты и sidecar metadata лежат под переданным filesystem root; bind mount делает данные переживающими контейнер. Без mount контейнер disposable. Нет replication/HA. | Отдельные persistent metadata/data directories, SQLite/Fjall/LMDB metadata engines, replication/deduplication/compression. Single-node с replication factor 1 не имеет redundancy и официально не рекомендуется для production. |
| Auth | Опциональная одна пара access/secret через CLI (`SimpleAuth`, SigV4). Если credentials не задать, endpoint анонимно открыт; bucket-level permissions нет. | SigV4 access keys и собственные read/write/owner permissions на buckets; default key/bucket можно создать при старте. Это ближе к production, но требует больше setup. |
| Лицензия | Apache-2.0. | AGPL-3.0. Для запуска неизменённого сервиса локально это обычно не мешает, но модификации/сетевое использование требуют отдельной лицензионной оценки. |
| Local dev/tests | Очень хороший disposable S3 boundary double: маленький, быстрый bootstrap и ровно это заявлено его назначением. Риск — experimental `0.x`, поэтому image нужно pin по версии/digest и держать контрактный smoke test. | Технически подходит и лучше проверяет production-like auth/storage behavior, но для текущего узкого контракта несёт лишние config, ports, state и cluster lifecycle. Полезен только если Garage рассматривается как реальный self-hosted provider. |

## Принятое решение

Для `make storage-check`/`integration-check` используется **pinned `s3s-fs
v0.17.0`** (`ghcr.io/s3s-project/s3s@sha256:565e16c104805fa9d129f22134ea12d309a634f57f73185a32b3ac4b7c38b049`).
Catalog сохраняет проверки upload → HEAD/checksum → delete; текущий boundary
test успешно пройден против этого digest. Pin и контрактный тест обязательны,
поскольку совместимость s3s заявлена best-effort, а не как полный S3.

**Garage сейчас не добавлять** в local/kind: он не даёт полезного покрытия сверх
узкого Catalog-контракта пропорционально своей операционной цене. Рассматривать
его отдельным ADR/PoC, только если нужен production-capable self-hosted S3. В
kind оставить существующий ephemeral in-process adapter, как уже решено в
Catalog ADR-0002; внешний S3 server нужен именно disposable boundary tests, а
не всему локальному runtime.

## Первичные источники

- s3s: [README и позиционирование](https://github.com/s3s-project/s3s/blob/v0.17.0/README.md), [Docker image/запуск](https://github.com/s3s-project/s3s/blob/v0.17.0/docs/docker.md), [s3s-fs CLI и auth](https://github.com/s3s-project/s3s/blob/v0.17.0/crates/s3s-fs/src/main.rs), [filesystem S3/checksum implementation](https://github.com/s3s-project/s3s/blob/v0.17.0/crates/s3s-fs/src/s3.rs), [v0.17.0 changelog](https://github.com/s3s-project/s3s/blob/v0.17.0/CHANGELOG.md), [Apache-2.0](https://github.com/s3s-project/s3s/blob/v0.17.0/LICENSE).
- Garage: [README и production history](https://git.deuxfleurs.fr/Deuxfleurs/garage/src/tag/v2.4.1/README.md), [single-node quick start](https://garagehq.deuxfleurs.fr/documentation/quick-start/), [S3 compatibility matrix](https://garagehq.deuxfleurs.fr/documentation/reference-manual/s3-compatibility/), [features](https://garagehq.deuxfleurs.fr/documentation/reference-manual/features/), [known issues](https://garagehq.deuxfleurs.fr/documentation/reference-manual/known-issues/), [v2.4.1 release](https://git.deuxfleurs.fr/Deuxfleurs/garage/releases/tag/v2.4.1), [checksum implementation](https://git.deuxfleurs.fr/Deuxfleurs/garage/src/tag/v2.4.1/src/api/common/signature/checksum.rs), [AGPL-3.0](https://git.deuxfleurs.fr/Deuxfleurs/garage/src/tag/v2.4.1/LICENSE).
- Gavel: [Catalog ADR-0002](../contexts/catalog/adr/0002-s3-compatible-media-storage.md), [S3 adapter](../contexts/catalog/src/adapters/outbound/s3.rs), [current disposable storage target](../contexts/catalog/Makefile).
