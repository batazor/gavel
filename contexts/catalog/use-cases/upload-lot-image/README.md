# Загрузить изображение лота

Контекст: Catalog · Актор: продавец · Диаграмма: [`flow.c4`](./flow.c4) → `flow_upload_lot_image`

## Цель

Продавец загружает изображение до создания лота. Бинарные данные идут напрямую в S3-compatible storage, а Catalog контролирует identity, object key и жизненный цикл загрузки.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    actor Seller as Продавец
    participant Web
    participant Router as Cosmo Router
    participant Catalog
    participant DB as catalog-db
    participant S3 as S3-compatible storage

    Seller->>Web: Выбрать изображение
    Web->>Router: requestLotImageUpload<br/>(type, length, checksumSha256)
    Router->>Catalog: Request upload
    Catalog->>Catalog: Validate metadata<br/>generate imageId + opaque object key
    Catalog->>DB: Claim idempotency key<br/>and save PENDING intent
    alt Exact completed replay
        DB-->>Catalog: Stored safe result
        Catalog-->>Router: Same upload intent
    else Changed payload for the same key
        DB-->>Catalog: Idempotency conflict
        Catalog-->>Router: Stable conflict
    else New or retryable PENDING intent
        DB-->>Catalog: Durable PENDING with the same key
        Catalog->>S3: Presign bounded PUT
        alt Storage is available
            S3-->>Catalog: URL + required headers
            Catalog->>DB: Store replay-safe result
            Catalog-->>Router: imageId + presigned PUT + headers
        else Presign timeout or failure
            Catalog-->>Router: Retryable dependency error
            Note over Catalog,DB: PENDING remains durable; retry reuses imageId and object key
        end
    end
    Router-->>Web: Upload intent
    Web->>S3: PUT bytes directly<br/>with x-amz-checksum-sha256
    S3-->>Web: Stored
    Web->>Router: completeLotImageUpload(imageId)
    Router->>Catalog: Complete upload
    alt Image is already READY
        Catalog-->>Web: Same imageId, READY
    else Image is PENDING
        Catalog->>S3: Bounded HEAD object
        S3-->>Catalog: Size, MIME type, checksum
        Catalog->>Catalog: Verify expected metadata and owner
        alt Metadata matches before expiry
            Catalog->>DB: Atomic PENDING → READY
            Catalog-->>Web: imageId, READY
        else Missing, mismatched or expired
            Catalog->>DB: Mark INVALID and cleanup-eligible
            Catalog-->>Web: Stable rejection
        end
    end
    loop Lease-based reconciliation
        Catalog->>DB: Lease expired PENDING / orphan READY / INVALID
        Catalog->>S3: Idempotent DELETE
        Catalog->>DB: Complete cleanup or release lease for retry
    end
    Note over Web,S3: Storage credentials and internal object key are never exposed
```

## Ошибки и граничные случаи

- Presigned URL ограничен одним object key, методом `PUT`, ожидаемыми headers и коротким TTL.
- Несовпадение размера, типа или checksum оставляет загрузку непригодной для прикрепления; объект удаляется lease-based reconciliation.
- Ошибка presign не теряет durable `PENDING`: exact retry использует прежние `imageId` и object key, а изменённый payload с тем же ключом получает conflict.
- Повторный `completeLotImageUpload` идемпотентен и возвращает текущее состояние.
- `imageId` другого продавца, уже прикреплённое или просроченное изображение отвергается.
- Если клиент не завершил сценарий, lifecycle/reconciliation удаляет объект и metadata после TTL.
- На момент `ListLot` требуется 1–20 готовых изображений.

## Транзакционные границы

S3-compatible storage и PostgreSQL не объединяются распределённой транзакцией. Intent фиксируется до presign, а `READY` — только после успешной сверки HEAD. Upload завершается до создания лота; PostgreSQL-транзакция атомарно связывает уже готовые изображения с лотом и outbox. Утечки между шагами устраняются идемпотентностью, lease и reconciliation; `ATTACHED` никогда не выбирается для автоматического удаления.
