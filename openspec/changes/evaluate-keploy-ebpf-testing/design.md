# Design

## Context

См. `proposal.md` — Why. Local MVP уже запускает восемь owner workloads и реальные PostgreSQL, Redis, Redpanda, SpiceDB и Temporal в выделенном `kind`-кластере `gavel-mvp`; `make mvp-smoke` и `make mvp-e2e` остаются контрольной группой. Keploy перехватывает сокеты через eBPF только на Linux. На macOS native mode использует userspace interception и не подходит для проверки заявленного eBPF-пути; Linux находится внутри Docker Desktop и `kind` nodes.

Kubernetes-интеграция Keploy может потребовать sidecar/webhook или node-level DaemonSet, повышенные capabilities, внешний control plane и учётную запись. Эти требования меняются быстрее кода Gavel, поэтому до установки нужен явный compatibility/security gate. Пилот не имеет права отправлять recording artifacts, credentials или пользовательские данные во внешний сервис без отдельного принятого решения.

## Goals / Non-Goals

**Goals:**

- Проверить реальный eBPF capture на Linux для существующего Rust HTTP/GraphQL workload, а не только запуск CLI.
- Доказать, что записанный public read можно повторить без реальных downstream dependencies и получить стабильное сравнение ответа.
- Получить измеримые критерии `adopt`, `defer` или `reject`, включая setup cost, privileges, протокольное покрытие, детерминизм и обработку чувствительных данных.
- Сохранить пилот opt-in, полностью удаляемым и не влияющим на обычный local MVP.

**Non-Goals:**

- Превращать Keploy в замену существующим Rust, contract, Playwright, smoke или k6 тестам.
- Записывать protected mutations, платежи, ставки или production traffic в первой итерации.
- Включать Keploy в production manifests либо обязательный PR gate до принятия итогового ADR.

## Decisions

### 1. Пилот разделяется на compatibility gate и record/replay

Сначала скрипт в `deploy/keploy/` проверяет pinned version, host architecture, Linux kernel/capabilities фактического agent runtime, условия лицензии/аккаунта, место хранения артефактов и возможность полностью локального replay. Результат выводится без установки cluster-wide компонентов.

Только при прохождении gate выполняются запись и replay. Если выбранная версия требует обязательной выгрузки capture data в SaaS, недоступных лицензий или непропорциональных cluster-wide privileges, эксперимент завершается документированным `defer`/`reject`; обходить ограничение самодельным packet proxy нельзя.

Альтернатива — сразу установить актуальный Helm chart — отклонена: она смешивает проверку продукта с необратимо широкими RBAC/capability изменениями и не даёт раннего stop condition.

### 2. Первый system under test — Catalog public GraphQL read в local `kind`

Запись нацеливается на Catalog deployment и один public query существующего seeded lot. Catalog выбран потому, что это Rust workload с HTTP/GraphQL ingress и PostgreSQL dependency, но public read не требует JWT, bid maximum, payment data или изменения доменного состояния. Трафик генерирует отдельный `deploy/keploy/record-catalog.sh`; health, readiness, metrics и фоновые worker calls исключаются фильтрами.

Запись через BFF отклонена для первого пилота: Cosmo Router добавил бы federation fan-out и динамические служебные поля раньше, чем доказана базовая совместимость Keploy с Rust/PostgreSQL. Protected Catalog mutation оставлена последующим экспериментом только после успешной санитарной проверки public read fixtures.

### 3. Sidecar mode предпочтителен DaemonSet mode, но только в экспериментальном overlay

Для dev-кластера допустим контролируемый rollout одного Catalog deployment, поэтому sidecar ограничивает blast radius лучше node-level DaemonSet. Keploy manifests/values хранятся только под `deploy/keploy/`; service-owned chart `contexts/catalog/ops/Helm` и production values не изменяются. Overlay должен иметь однозначные команды install/uninstall и labels, по которым можно проверить полное удаление ресурсов.

Если pinned community/free distribution не поддерживает локальный Kubernetes sidecar record/replay, допустим Docker-based Linux agent как запасной путь при том же Catalog image и том же запросе. Он считается успешным eBPF-пилотом только если gate доказывает, что hooks загружены в Linux kernel, а не используется macOS userspace capture. Переход на DaemonSet или платный Kubernetes proxy требует отдельного решения и не является автоматическим fallback.

### 4. Fixtures коммитятся только после fail-closed sanitation

Allowlist для committed artifact включает request method/path/body, безопасные response fields и минимальные dependency mocks. Проверка в `deploy/keploy/tests/fixture-safety.sh` падает при Bearer/JWT, cookies, authorization headers, opaque principal ids, bid maxima, provider/storage references, URLs с credentials, Authz revisions и известных local secrets. Reports, tokens, runtime state и raw captures игнорируются.

Запись является ручной и намеренно перезаписывает только явно выбранный test set. Replay читает version-controlled fixture read-only. Это отделяет создание golden data от обычной проверки и предотвращает «самообновление» expected результата при регрессии.

### 5. Успех оценивается воспроизводимостью и способностью поймать регрессию

Пилот считается технически успешным, если:

1. gate подтверждает Linux eBPF path и документирует выданные privileges;
2. fixture safety check проходит;
3. replay проходит три раза подряд после остановки/изоляции записанных downstream dependencies;
4. контролируемое изменение ожидаемого response приводит к падению replay, после чего fixture восстанавливается;
5. median replay и setup time, размер fixtures и ручные шаги записаны в `deploy/keploy/RESULTS.md`.

Решение `adopt` принимается только если все пять условий выполнены, replay можно запускать одной Make-командой и нет обязательной передачи capture data наружу. `Defer` используется для временного upstream/kernel/licensing ограничения; `reject` — для фундаментально неподходящей модели безопасности, нестабильных повторов или отсутствия полезного покрытия сверх текущих тестов.

### 6. Consistency, authorization, recovery и observability остаются вне runtime

Public read не создаёт domain state, поэтому idempotency сводится к неизменяемому fixture и повторяемому seed id. Авторизация не обходится: пилот выбирает уже публичный query, а не подделывает verified identity. Ошибка установки или записи вызывает cleanup экспериментальных ресурсов и не меняет `gavel-mvp`; `make mvp-ready` после cleanup подтверждает восстановление. Логи пилота содержат только bounded phase/outcome и не сохраняют raw payload по умолчанию.

## Risks / Trade-offs

- **[Keploy Kubernetes path доступен только через SaaS/enterprise]** → compatibility gate останавливает установку; результат фиксируется как `defer` или `reject`, Docker fallback оценивается отдельно и не маскирует ограничение Kubernetes.
- **[eBPF capabilities расширяют доступ к node traffic]** → sidecar одного workload, отдельный namespace/service account где возможно, минимальные capabilities, отсутствие production deployment и полный uninstall check.
- **[Dynamic ids/timestamps делают golden response нестабильным]** → один public read seeded fixture, explicit noise rules и три последовательных replay; нестабильное поле не удаляется без объяснения в results.
- **[Запись захватывает фоновые соединения Catalog]** → allowlist ingress query и фильтры health/metrics/workers; raw capture не коммитится до sanitation.
- **[Пилот проходит, но не даёт нового покрытия]** → сравнить обнаруженные зависимости и дефекты с существующим `catalog_boundary_flow` и `mvp-smoke`; отсутствие дополнительной ценности является основанием `reject`.
- **[Upstream CLI/config быстро меняются]** → pin exact version/digest и хранить источник/version рядом с scripts; upgrade не происходит автоматически.

## Migration Plan

1. Добавить статические prerequisites/safety checks и документацию, не устанавливая Keploy.
2. Выполнить compatibility gate и зафиксировать фактическую среду.
3. При успехе установить только experimental overlay, записать Catalog fixture, очистить cluster и проверить `make mvp-ready`.
4. Запустить изолированный replay и negative control, заполнить `RESULTS.md`.
5. Обновить `VISION.md` и delivery ADR итогом `adopt`, `defer` или `reject`.

Rollback состоит из `make keploy-down`, удаления только ресурсов с experiment labels и удаления локальных runtime artifacts. Existing fixtures остаются в Git для review; если решение `reject`, отдельная финальная правка удаляет неиспользуемые executable hooks, сохраняя ADR с выводом.

## Open Questions

- Конкретная pinned-версия и image digest выбираются при implementation после проверки текущего release manifest и фиксируются до первой записи.
- Точные kernel capabilities зависят от выбранного sidecar image и фактического kernel Docker Desktop; gate сохранит их в `RESULTS.md` без секретов.
