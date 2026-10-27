{{- define "qovery-storage-class.renderable" -}}
{{- $existing := lookup "storage.k8s.io/v1" "StorageClass" "" .name -}}
{{- $annotations := dig "metadata" "annotations" dict $existing -}}
{{- $labels := dig "metadata" "labels" dict $existing -}}
{{- $ownedByRelease := and (eq (get $labels "app.kubernetes.io/managed-by") "Helm") (eq (get $annotations "meta.helm.sh/release-name") .root.Release.Name) (eq (get $annotations "meta.helm.sh/release-namespace") .root.Release.Namespace) -}}
{{- if or (not $existing) $ownedByRelease -}}
true
{{- end -}}
{{- end -}}
