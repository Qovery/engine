{{/*
See, if we can upgrade the mutatingWebhookConfiguration
*/}}
{{- define "vpa.webhook.upgradable" -}}
{{/*lookup config*/}}
{{- $webhook := (lookup "admissionregistration.k8s.io/v1" "MutatingWebhookConfiguration" "" (printf "%s-%s" (include "vpa.fullname" .) "webhook-config")) }}
{{- if $webhook }}
  {{- /*is it managed by this helm release?*/ -}}
  {{- if and
    (hasKey $webhook.metadata "labels")
    (hasKey $webhook.metadata "annotations")
    (hasKey $webhook.metadata.labels "app.kubernetes.io/managed-by")
    (hasKey $webhook.metadata.annotations "meta.helm.sh/release-name")
    (hasKey $webhook.metadata.annotations "meta.helm.sh/release-namespace")
    (eq (get $webhook.metadata.labels "app.kubernetes.io/managed-by") "Helm")
    (eq (get $webhook.metadata.annotations "meta.helm.sh/release-name") .Release.Name)
    (eq (get $webhook.metadata.annotations "meta.helm.sh/release-namespace") .Release.Namespace)
  }}
    {{- "true" | toYaml -}}
  {{- else }}
    {{- "" -}}
  {{- end }}
{{- else }}
  {{- "true" | toYaml -}}
{{- end }}
{{- end }}

{{/*
Return the name for the webhook tls secret
*/}}
{{- define "vpa.webhook.secret" -}}
{{- if .Values.admissionController.secretName }}
{{- default (printf "%s-%s" (include "vpa.fullname" .) "tls-certs") (tpl (.Values.admissionController.secretName | toString) .) }}
{{- else }}
{{- printf "%s-%s" (include "vpa.fullname" .) "tls-certs" }}
{{- end }}
{{- end }}
