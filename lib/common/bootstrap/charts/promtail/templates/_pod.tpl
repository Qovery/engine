{{/*
Pod template used in Daemonset and Deployment
*/}}
{{- define "promtail.podTemplate" -}}
metadata:
  labels:
    {{- include "promtail.selectorLabels" . | nindent 4 }}
    {{- with .Values.podLabels }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
  annotations:
    {{- if not .Values.sidecar.configReloader.enabled }}
    checksum/config: {{ include (print .Template.BasePath "/secret.yaml") . | sha256sum }}
    {{- end }}
    {{- with .Values.podAnnotations }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
spec:
  serviceAccountName: {{ include "promtail.serviceAccountName" . }}
  {{- include "promtail.enableServiceLinks" . | nindent 2 }}
  {{- with .Values.priorityClassName }}
  priorityClassName: {{ . }}
  {{- end }}
  {{- with .Values.initContainer }}
  initContainers:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  {{- with .Values.imagePullSecrets }}
  imagePullSecrets:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  securityContext:
    {{- toYaml .Values.podSecurityContext | nindent 4 }}
  containers:
    - name: promtail
      image: "{{ .Values.image.registry }}/{{ .Values.image.repository }}:{{ .Values.image.tag | default .Chart.AppVersion }}"
      imagePullPolicy: {{ .Values.image.pullPolicy }}
      args:
        - "-config.file=/etc/promtail/promtail.yaml"
        {{- if .Values.sidecar.configReloader.enabled }}
        - "-server.enable-runtime-reload"
        {{- end }}
        {{- with .Values.extraArgs }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      volumeMounts:
        - name: config
          mountPath: /etc/promtail
        {{- with .Values.defaultVolumeMounts }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
        {{- with .Values.extraVolumeMounts }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      env:
        - name: HOSTNAME
          valueFrom:
            fieldRef:
              fieldPath: spec.nodeName
      {{- with .Values.extraEnv }}
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.extraEnvFrom }}
      envFrom:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      ports:
        - name: http-metrics
          containerPort: {{ .Values.config.serverPort }}
          protocol: TCP
        {{- range $key, $values := .Values.extraPorts }}
        - name: {{ .name | default $key }}
          containerPort: {{ $values.containerPort }}
          protocol: {{ $values.protocol | default "TCP" }}
        {{- end }}
      securityContext:
        {{- toYaml .Values.containerSecurityContext | nindent 8 }}
      {{- with .Values.livenessProbe }}
      livenessProbe:
        {{- tpl (toYaml .) $ | nindent 8 }}
      {{- end }}
      {{- with .Values.readinessProbe }}
      readinessProbe:
        {{- tpl (toYaml .) $ | nindent 8 }}
      {{- end }}
      {{- with .Values.resources }}
      resources:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    {{- if .Values.sidecar.configReloader.enabled }}
    - name: config-reloader
      image: "{{ .Values.sidecar.configReloader.image.registry }}/{{ .Values.sidecar.configReloader.image.repository }}:{{ .Values.sidecar.configReloader.image.tag }}"
      imagePullPolicy: {{ .Values.sidecar.configReloader.image.pullPolicy }}
      args:
        - '-web.listen-address=:{{ .Values.sidecar.configReloader.config.serverPort }}'
        - '-volume-dir=/etc/promtail/'
        - '-webhook-method=GET'
        - '-webhook-url=http://127.0.0.1:{{ .Values.config.serverPort }}/reload'
      {{- range .Values.sidecar.configReloader.extraArgs }}
        - {{ . }}
      {{- end }}
      ports:
        - name: reloader
          containerPort: {{ .Values.sidecar.configReloader.config.serverPort }}
          protocol: TCP
      {{- with .Values.sidecar.configReloader.extraEnv }}
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.sidecar.configReloader.extraEnvFrom }}
      envFrom:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- toYaml .Values.sidecar.configReloader.containerSecurityContext | nindent 8 }}
      {{- with .Values.sidecar.configReloader.livenessProbe }}
      livenessProbe:
        {{- tpl (toYaml .) $ | nindent 8 }}
      {{- end }}
      {{- with .Values.sidecar.configReloader.readinessProbe }}
      readinessProbe:
        {{- tpl (toYaml .) $ | nindent 8 }}
      {{- end }}
      {{- with .Values.sidecar.configReloader.resources }}
      resources:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      volumeMounts:
        - name: config
          mountPath: /etc/promtail
    {{- end }}
    {{- if .Values.extraContainers }}
    {{- range $name, $values := .Values.extraContainers }}
    - name: {{ $name }}
      {{ toYaml $values | nindent 6 }}
    {{- end }}
    {{- end }}
  {{- with .Values.affinity }}
  affinity:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  {{- with .Values.nodeSelector }}
  nodeSelector:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  {{- with .Values.tolerations }}
  tolerations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  volumes:
    - name: config
      {{- if .Values.configmap.enabled }}
      configMap:
        name: {{ include "promtail.fullname" . }}
      {{- else }}
      secret:
        secretName: {{ include "promtail.fullname" . }}
      {{- end }}
    {{- with .Values.defaultVolumes }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
    {{- with .Values.extraVolumes }}
    {{- toYaml . | nindent 4 }}
    {{- end }}
{{- end }}
