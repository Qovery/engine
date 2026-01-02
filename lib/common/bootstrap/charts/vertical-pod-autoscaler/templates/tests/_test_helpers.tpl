{{/*
Get kubectl image tag
*/}}
{{- define "vpa.test.tag" -}}
{{- if .Values.tests.image }}
{{- default (printf "%s.%s" .Capabilities.KubeVersion.Major .Capabilities.KubeVersion.Minor) .Values.tests.image.tag }}
{{- else }}
{{- printf "%s.%s" .Capabilities.KubeVersion.Major .Capabilities.KubeVersion.Minor }}
{{- end }}
{{- end }}

{{/*
Get kubectl image name
*/}}
{{- define "vpa.test.image" -}}
{{- if .Values.tests.image }}
{{- printf "%s:%s" (default "bitnami/kubectl" .Values.tests.image.repository) (default (include "vpa.test.tag" . ) .Values.tests.image.tag) }}
{{- else }}
{{- printf "bitnami/kubectl:%s" (include "vpa.test.tag" . ) }}
{{- end }}
{{- end }}
