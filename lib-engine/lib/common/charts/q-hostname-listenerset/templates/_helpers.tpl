{{/* Labels shared by every resource of the release. The release name depends on the hostname only,
  the service-id label records the router that applied it. */}}
{{- define "qovery.hostnameListenerSetLabels" -}}
qovery.com/service-id: {{ toJson .Values.router_long_id }}
qovery.com/service-type: "router"
qovery.com/router-name: {{ toJson .Values.router_name }}
qovery.com/environment-id: {{ toJson .Values.environment_long_id }}
qovery.com/project-id: {{ toJson .Values.project_long_id }}
qovery.com/listenerset-scope: "hostname"
{{- end -}}
