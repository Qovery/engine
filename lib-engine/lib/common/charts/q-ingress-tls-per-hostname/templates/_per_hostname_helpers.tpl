{{/*
  PER_HOSTNAME_LISTENER_SET: Gateway API hostnames are lowercase without a trailing dot, and the ListenerSet
  hostnames are normalized the same way (trim, lowercase, remove one trailing dot, as `normalize_hostname` does in Rust).
*/}}
{{- define "qovery.perHostnameRouteHostname" -}}
{{- trimSuffix "." (lower (trim .)) -}}
{{- end -}}
