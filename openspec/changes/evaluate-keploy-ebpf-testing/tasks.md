# Tasks

## 1. Shared delivery: compatibility и безопасный scaffold

- [x] 1.1 Проверить актуальные official Keploy release/install/Kubernetes материалы, выбрать exact CLI version и agent image digest, записать источник, лицензионный/аккаунтный режим и supported kernel matrix в `deploy/keploy/README.md`; проверить, что указанные version/digest разрешаются и документация не обещает macOS userspace mode как eBPF.
- [x] 1.2 Добавить `deploy/keploy/check.sh` с fail-closed проверками обязательных команд, architecture, фактического Linux kernel agent runtime, eBPF/capability prerequisites, local artifact storage и отсутствия неявной SaaS upload; покрыть ветки supported/unsupported shell contract tests и прогнать их без изменения кластера.
- [x] 1.3 Добавить в корневой `Makefile` discoverable opt-in цели `keploy-check`, `keploy-up`, `keploy-down`, `keploy-record` и `keploy-replay`, а в `deploy/keploy/README.md` описать lifecycle и stop conditions; проверить цели через `make help` и dry-run/contract test.
- [x] 1.4 Добавить fail-closed `deploy/keploy/tests/fixture-safety.sh`, denylist чувствительных Gavel полей/секретов и `.gitignore` для tokens, reports, raw captures и runtime state; доказать тестовыми safe/unsafe fixtures, что безопасный fixture проходит, а JWT, Authorization, credentials, bid maxima, provider refs и Authz revisions отклоняются.

## 2. Shared delivery + Catalog: изолированный record path

- [x] 2.1 Реализовать экспериментальный sidecar overlay под `deploy/keploy/` для одного Catalog deployment с pinned image, минимальными RBAC/capabilities, experiment labels и идемпотентным uninstall; если compatibility gate докажет отсутствие допустимого local/free sidecar path, вместо установки зафиксировать evidence-backed `defer`/`reject` в `RESULTS.md` и проверить, что cluster не изменён.
- [x] 2.2 Добавить `record-catalog.sh`, который использует существующий `gavel-mvp` seed и записывает только один public Catalog GraphQL read, исключая health/readiness/metrics и фоновые calls; проверить generated artifacts safety gate до любого копирования в version-controlled fixtures.
- [x] 2.3 Сохранить минимальный sanitized Catalog test set и dependency mocks read-only либо, при stop condition, явно зафиксировать отсутствие fixtures и причину; проверить `git diff` и safety script на всём коммитимом каталоге.
- [ ] 2.4 Выполнить `make keploy-down`, проверить отсутствие experiment-labeled Keploy resources и затем `make mvp-ready`, чтобы доказать восстановление local MVP; команды и результаты записать в `RESULTS.md`.

## 3. Shared delivery: replay и решение эксперимента

- [x] 3.1 Реализовать изолированный `make keploy-replay`, который не изменяет golden fixtures и использует recorded mocks без реальных recorded downstream dependencies; либо, если stop condition уже сработал, сделать цель fail-fast с понятной ссылкой на `RESULTS.md`.
- [x] 3.2 При наличии fixtures прогнать replay три раза подряд, измерить setup/replay time и размер artifacts; выполнить negative control с временно изменённым expected response и доказать падение без сохранения мутации. При отсутствии fixtures приложить к `RESULTS.md` воспроизводимое доказательство блокирующего ограничения.
- [x] 3.3 Сравнить полученное покрытие и стоимость с `contexts/catalog/tests/catalog_boundary_flow.rs` и `make mvp-smoke`, заполнить в `deploy/keploy/RESULTS.md` критерии и одно итоговое решение `adopt`, `defer` или `reject`; проверить, что документ не содержит credentials, raw payload или machine-specific paths.
- [x] 3.4 Обновить пункт Keploy в `VISION.md` и `docs/adr/0007-delivery-kind-helm-argocd-lean-ci.md` фактическим решением и границами применения; подтвердить, что GraphQL SDL, events, authorization, use-case Mermaid и LikeC4 runtime views не менялись, потому что пилот не изменяет runtime contracts.
- [ ] 3.5 Прогнать `make keploy-check`, fixture safety tests, соответствующий replay/stop-path check, `openspec validate --all` и `make mvp-ready`; все недоступные проверки и причины записать в `RESULTS.md`.

## Workflow follow-up

- После review результата синхронизировать итоговые capability specs не требуется: change использует `skip_specs: true` и не меняет поведение продукта.
- Архивировать change только после фиксации решения `adopt`, `defer` или `reject` и проверки cleanup.
