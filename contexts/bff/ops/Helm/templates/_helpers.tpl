{{- define "gavel.name" -}}
{{- .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "gavel.fullname" -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "gavel.labels" -}}
app.kubernetes.io/name: {{ include "gavel.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: gavel
{{- end -}}

{{- define "gavel.localAuthGuard" -}}
{{- if and .Values.localAuth.enabled (ne .Values.deploymentProfile "local") -}}
{{- fail "localAuth.enabled requires deploymentProfile=local; development issuer must never run in production" -}}
{{- end -}}
{{- if .Values.localAuth.enabled -}}
{{- $issuer := required "localAuth.issuer is required" .Values.localAuth.issuer -}}
{{- $audience := required "localAuth.audience is required" .Values.localAuth.audience -}}
{{- $jwksUri := required "localAuth.jwksUri is required" .Values.localAuth.jwksUri -}}
{{- end -}}
{{- end -}}
