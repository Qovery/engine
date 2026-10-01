{{/* Preserve the indentation of multiline nginx snippets after their first line. */}}
{{- define "qovery.indentAfterFirst" -}}
{{- $value := index . 0 -}}
{{- $indent := int (index . 1) -}}
{{- replace "\n" (printf "\n%s" (repeat $indent " ")) $value -}}
{{- end -}}

{{/* Only characters that are inert in an nginx header-name token are allowed. */}}
{{- define "qovery.nginxHeaderName" -}}
{{- regexReplaceAll "[^a-zA-Z0-9_.!%&*+^|~-]" . "" -}}
{{- end -}}

{{/* Escape quoted nginx values and remove controls that could leave the YAML block. */}}
{{- define "qovery.nginxHeaderValue" -}}
{{- regexReplaceAll "[\\x00-\\x1f\\x7f-\\x9f]" . "" | replace "\\" "\\\\" | replace "\"" "\\\"" | replace "'" "\\'" | replace "{{" "{ {" -}}
{{- end -}}

{{/* Match the legacy first non-null rewrite, including an explicitly empty string. */}}
{{- define "qovery.firstPathRewrite" -}}
{{- $found := false -}}
{{- range . -}}
{{- if and (not $found) (ne .path_rewrite nil) -}}
{{- .path_rewrite -}}
{{- $found = true -}}
{{- end -}}
{{- end -}}
{{- end -}}
