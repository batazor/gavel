# 2. S3-compatible storage for lot media

Date: 2026-10-22

## Status

Accepted

## Контекст

Фотографии лотов — крупные бинарные объекты с отдельным профилем нагрузки и жизненным циклом. Хранить их в PostgreSQL или передавать через Cosmo Router и catalog-service дорого: это увеличивает размер бэкапов, расход памяти приложений и трафик API data plane.

Catalog при этом должен владеть связью фотографии с продавцом и лотом, порядком изображений, alt-текстом и состоянием загрузки. Публичный интерфейс не должен принимать произвольные внешние URL, иначе Gavel не контролирует доступность, целостность и безопасность содержимого карточки.

## Решение

Catalog использует object storage через **S3-compatible interface**. Production runtime использует S3 SigV4 adapter; kind включает отдельный ephemeral in-process adapter тем же application port. Pinned `s3s-fs` используется как disposable S3 boundary dependency интеграционных тестов, но не является workload локального MVP, частью доменной модели или production provider.

- Бинарные объекты хранятся в S3-compatible storage под непрозрачными server-generated keys.
- `catalog-db` хранит `image_id`, владельца, object key, ожидаемые MIME type/size/checksum, состояние `PENDING | READY | ATTACHED`, порядок и alt-текст. Credentials, presigned URL и публичный URL в доменном состоянии не сохраняются.
- Web сначала вызывает `requestLotImageUpload`, затем делает прямой `PUT` по короткоживущему presigned URL и подтверждает загрузку через `completeLotImageUpload`.
- Catalog проверяет object metadata и checksum через S3-compatible adapter. До `READY` объект нельзя прикрепить к лоту или отдать на чтение.
- `ListLot` принимает только `imageId`, принадлежащие текущему продавцу и находящиеся в `READY`. При создании лота они становятся `ATTACHED` в той же PostgreSQL-транзакции, что лот и `LotListed` outbox.
- Между object storage и PostgreSQL нет распределённой транзакции. Просроченные `PENDING` и не прикреплённые `READY` объекты удаляет reconciliation/lifecycle job.
- На один лот допускается от 1 до 20 изображений. Разрешённые форматы, максимальный размер, проверка декодирования и удаление опасных metadata задаются политикой Catalog, а не возможностями конкретного provider.
- Чтение идёт по URL, сформированному delivery adapter. Object key и внутренний endpoint provider наружу не раскрываются.

Интерфейс object-storage модуля остаётся узким: создать upload capability, получить metadata объекта и удалить объект. Особенности local adapter/s3s-fs/AWS S3/Ceph RGW скрыты за adapter.

Первый adapter подписывает S3 SigV4 сам и переиспользует один HTTP connection pool. Presigned PUT подписывает `content-type` и `x-amz-checksum-sha256`, возвращает обязательный `content-length`, ограничивает срок семью днями и никогда не включает secret key в URL или ошибки. `HEAD` запрашивает checksum mode, а cleanup считает успешными delete и already-missing object.

## Последствия

- В production бинарный upload не проходит через Router или catalog-service; они управляют только намерением и состоянием загрузки. В kind одноразовый local-only upload capability завершается в Catalog, чтобы не поднимать тяжёлый storage workload.
- Смена test double или production provider на AWS S3, Ceph RGW либо другое S3-compatible storage не меняет GraphQL schema и доменную модель.
- `s3s-fs` остаётся только integration-test dependency: production implementation должна отдельно обеспечить durability, encryption, lifecycle rules, observability и availability.
- Публичные URL объектов считаются производными delivery-данными и могут позже указывать на CDN без миграции домена.

## Референсы

- [Catalog](../README.md)
- [Загрузка изображения](../use-cases/upload-lot-image/README.md)
- [Catalog subgraph SDL](../graphql/schema.graphql)
- [Сравнение локальных S3 implementations](../../../docs/local-s3-implementations.md)
- [ADR-0013 — Workload and capacity model](../../../docs/adr/0013-workload-and-capacity-model.md)
- [ADR-0014 — Availability, replication and sharding](../../../docs/adr/0014-availability-replication-and-sharding.md)
