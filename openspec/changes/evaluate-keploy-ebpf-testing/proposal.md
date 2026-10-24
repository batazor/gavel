# Proposal

## Why

В `VISION.md` Keploy зафиксирован как эксперимент для записи и воспроизведения интеграционных тестов, но у проекта ещё нет воспроизводимого пилота и критериев, по которым инструмент можно принять или отклонить. Проверка нужна сейчас, пока уже существует полный local MVP в `kind`: на нём можно измерить реальную ценность eBPF record/replay для Rust-сервисов с PostgreSQL, Redis, gRPC и Redpanda, не подменяя эксперимент игрушечным примером.

## What Changes

- Добавить изолированный пилот Keploy с pinned-версией и явной проверкой Linux/eBPF prerequisites; macOS используется только как host для Linux-контейнеров/узлов и не считается нативным eBPF-режимом.
- Выбрать один bounded-context HTTP/GraphQL boundary как первый system under test и записать небольшой детерминированный набор запросов вместе с dependency mocks.
- Добавить отдельные команды записи и replay, не меняющие существующие `mvp-smoke` и `mvp-e2e`; recording остаётся ручным, replay становится воспроизводимым verification target только после успешного пилота.
- Защитить fixtures от credentials, JWT, opaque principal ids, bid maxima, provider references и других чувствительных или нестабильных значений; generated reports и временные данные не коммитить.
- Зафиксировать результат эксперимента: поддержанные протоколы, ограничения Rust/kind/macOS, время запуска, стабильность повторов и решение `adopt`, `defer` или `reject` с обновлением `VISION.md` и delivery ADR.
- Не менять доменную семантику, публичные GraphQL/gRPC/event contracts и существующие acceptance-тесты.

## Capabilities

### New Capabilities

Нет. Это ограниченный эксперимент с development tooling, а не новая пользовательская или runtime capability.

### Modified Capabilities

Нет. Требования существующих capabilities не меняются; change использует `skip_specs: true`.

## Impact

- **Primary owner:** shared delivery/platform; основные пути — `deploy/`, корневой `Makefile`, `.gitignore`, `docs/adr/` и `VISION.md`.
- **Impacted bounded contexts:** только выбранный в design пилотный владелец API как system under test; его доменный код, datastore ownership и контракты не изменяются. Остальные contexts затрагиваются лишь как реальные downstream dependencies существующего local MVP.
- **Dependencies:** Keploy CLI/agent и его Linux eBPF privileges; возможны Docker Desktop или `kind`-node constraints и интерактивная/CI аутентификация Keploy. Любая обязательная SaaS-зависимость, повышенные cluster privileges и лицензионные ограничения должны быть отражены в итоговом решении.
- **Compatibility/migration:** breaking changes и миграции отсутствуют. Пилот opt-in и не входит в обычный `make check`, пока не доказана воспроизводимость.
- **Out of scope:** запись production traffic, установка Keploy в production, замена unit/property/contract/Playwright/k6 тестов, тестирование всех сервисов, отправка секретов или пользовательских данных во внешнюю систему.
