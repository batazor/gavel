{{- define "gavel-observability.labels" -}}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: gavel
gavel.io/monitoring: "true"
{{- end -}}
