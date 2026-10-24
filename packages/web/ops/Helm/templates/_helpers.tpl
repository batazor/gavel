{{- define "gavel-web.name" -}}gavel-web{{- end -}}
{{- define "gavel-web.fullname" -}}{{ printf "%s-%s" .Release.Name (include "gavel-web.name" .) | trunc 63 | trimSuffix "-" }}{{- end -}}
{{- define "gavel-web.labels" -}}
app.kubernetes.io/name: {{ include "gavel-web.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: gavel
{{- end -}}
