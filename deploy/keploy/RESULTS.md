# Keploy pilot: результаты

Дата: 2026-10-24. Решение: **reject**.

## Среда

| Параметр | Значение |
|---|---|
| Host | macOS, arm64, Docker Desktop 29.4.1 |
| Agent runtime | Linux 6.12.76-linuxkit, `CONFIG_BPF_SYSCALL`, `CONFIG_DEBUG_INFO_BTF`, kprobes и uprobes включены |
| Keploy | v3.6.161, open-source сборка, без login |
| Привилегии agent | `--cap-add=BPF,PERFMON,NET_ADMIN,SYS_RESOURCE,SYS_PTRACE`, mounts `/sys/fs/bpf`, `/sys/fs/cgroup`, debugfs |

## Что проверено

1. **eBPF path работает в Docker mode.** Контрольный HTTP-сервер в контейнере на `keploy-network` записан через `keploy record`: три test case за один запуск. `keploy test` прошёл 3 из 3 за 15 s replay и 19 s полного запуска. Размер test set 36 KiB. Agent работал в Linux kernel Docker VM, а не в macOS userspace mode.
2. **Протокольное покрытие недостаточно для Catalog.** CLI open-source сборки прямо сообщает, что она поддерживает HTTP и MySQL. PostgreSQL, gRPC, HTTP/2 и Kafka доступны только в сборке keploy.io с аккаунтом. Catalog при старте требует PostgreSQL и gRPC-соединение с Authz, поэтому изолированный replay без реальных downstream dependencies невозможен.
3. **Kubernetes path недоступен локально.** Официальный `k8s-proxy` chart требует Enterprise plan, access key и регистрацию кластера в app.keploy.io. Sidecar overlay не устанавливался, `gavel-mvp` не изменялся.
4. **Внешняя передача данных.** Бинарник содержит endpoint `telemetry.keploy.io`, agent получает `INSTALLATION_ID`, а `disableTele` по умолчанию `false`. Для любого будущего запуска нужно `disableTele: true` в `keploy.yml`.

## Критерии design

| Критерий | Результат |
|---|---|
| Gate подтверждает Linux eBPF и фиксирует привилегии | выполнено |
| Fixture safety проходит | выполнено на safe/unsafe тестовых fixtures; Catalog fixtures отсутствуют |
| Три replay подряд без downstream | не выполнялось: stop condition по протоколам |
| Negative control | выполнено на уровне gate и fixture safety; replay negative control не выполнялся |
| Время, размер, ручные шаги | записаны для контрольного HTTP-сервиса |

Негативный контроль replay и три прогона Catalog не выполнялись, потому что fixtures Catalog нельзя записать без PostgreSQL и gRPC mocks.

## Сравнение с текущими тестами

`contexts/catalog/tests/catalog_boundary_flow.rs` уже проверяет реальные PostgreSQL и object storage boundaries, а `make mvp-smoke` проверяет сквозные сценарии через публичные границы. Open-source Keploy для Catalog добавил бы только HTTP-ответ без dependency mocks, что не даёт нового покрытия.

## Решение

**Reject.** Публичная Apache-2.0 сборка не поддерживает PostgreSQL, gRPC и Kafka, на которых построены все сервисы Gavel. Эти протоколы и Kubernetes-режим доступны только с аккаунтом keploy.io или Enterprise, а обязательная внешняя зависимость для тестовой инфраструктуры проекту не подходит. Нового покрытия сверх `catalog_boundary_flow` и `make mvp-smoke` инструмент не даёт.

Исполняемые hooks пилота удалены: gate, fixture safety, Make targets `keploy-*`. Этот файл остаётся как запись эксперимента.

## Проверки, выполненные до удаления

| Проверка | Результат |
|---|---|
| Contract tests gate, fixture safety и Make targets | passed |
| Compatibility gate | exit 3: open-source сборка не покрывает postgres и grpc |
| Replay stop path | fail-fast со ссылкой на этот файл |
| Cleanup | контейнеры `keploy-v3*`, сеть `keploy-network` и volume debugfs удалены; в `gavel-mvp` ничего не устанавливалось |
| `openspec validate --all` | passed, 18 из 18 |
| `make mvp-ready` | не выполнялся: кластер в пилоте не менялся |
