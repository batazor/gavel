# Web Helm chart

Chart разворачивает собранный Vite SPA как отдельный stateless workload. Browser использует только публичный Cosmo Router `/graphql`; прямых адресов owner subgraphs в image нет.

Local image компилируется с `VITE_DATA_MODE=connected`, Router `http://127.0.0.1:8080/graphql` и development-only issuer `http://127.0.0.1:8080/default`. Web и BFF публикуются отдельными port-forward: Web `3000:8080`, BFF/JWT ingress `8080:8080`.

Production build MUST передавать production URLs и держать local identity switcher выключенным. `values.yaml` не включает ingress автоматически; общий edge владеет host/path routing.
