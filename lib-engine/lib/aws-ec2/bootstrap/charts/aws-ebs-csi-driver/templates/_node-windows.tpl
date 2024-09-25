{{- define "node-windows" }}
{{- if .Values.node.enableWindows }}
---
kind: DaemonSet
apiVersion: apps/v1
metadata:
  name: {{ printf "%s-windows" .NodeName }}
  namespace: {{ .Values.node.namespaceOverride | default .Release.Namespace }}
  labels:
    {{- include "aws-ebs-csi-driver.labels" . | nindent 4 }}
spec:
  {{- if or (kindIs "float64" .Values.node.revisionHistoryLimit) (kindIs "int64" .Values.node.revisionHistoryLimit) }}
  revisionHistoryLimit: {{ .Values.node.revisionHistoryLimit }}
  {{- end }}
  selector:
    matchLabels:
      app: {{ .NodeName }}
      {{- include "aws-ebs-csi-driver.selectorLabels" . | nindent 6 }}
  updateStrategy:
    {{ toYaml .Values.node.updateStrategy | nindent 4 }}
  template:
    metadata:
      labels:
        app: {{ .NodeName }}
        {{- include "aws-ebs-csi-driver.labels" . | nindent 8 }}
        {{- if .Values.node.podLabels }}
        {{- toYaml .Values.node.podLabels | nindent 8 }}
        {{- end }}
      {{- with .Values.node.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      {{- with .Values.node.affinity }}
      affinity: {{- toYaml . | nindent 8 }}
      {{- end }}
      nodeSelector:
        kubernetes.io/os: windows
        {{- with .Values.node.nodeSelector }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      serviceAccountName: {{ .Values.node.serviceAccount.name }}
      terminationGracePeriodSeconds: {{ .Values.node.terminationGracePeriodSeconds }}
      priorityClassName: {{ .Values.node.priorityClassName | default "system-node-critical" }}
      tolerations:
        {{- if .Values.node.tolerateAllTaints }}
        - operator: Exists
        {{- else }}
        {{- with .Values.node.tolerations }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
        {{- end }}
      {{- if .Values.node.windowsHostProcess }}
      securityContext:
        windowsOptions:
          hostProcess: true
          runAsUserName: "NT AUTHORITY\\SYSTEM"
      hostNetwork: true
      {{- end }}
      containers:
        - name: ebs-plugin
          image: {{ printf "%s%s:%s" (default "" .Values.image.containerRegistry) .Values.image.repository (default (printf "v%s" .Chart.AppVersion) (toString .Values.image.tag)) }}
          imagePullPolicy: {{ .Values.image.pullPolicy }}
          {{- if .Values.node.windowsHostProcess }}
          command:
            - "aws-ebs-csi-driver.exe"
          {{- end }}
          args:
            - node
            - --endpoint=$(CSI_ENDPOINT)
            {{- with .Values.node.reservedVolumeAttachments }}
            - --reserved-volume-attachments={{ . }}
            {{- end }}
            {{- with .Values.node.volumeAttachLimit }}
            - --volume-attach-limit={{ . }}
            {{- end }}
            {{- if .Values.node.legacyXFS }}
            - --legacy-xfs=true
            {{- end}}
            {{- with .Values.node.loggingFormat }}
            - --logging-format={{ . }}
            {{- end }}
            - --v={{ .Values.node.logLevel }}
            {{- if .Values.node.otelTracing }}
            - --enable-otel-tracing=true
            {{- end}}
            {{- if .Values.node.windowsHostProcess }}
            - --windows-host-process=true
            {{- end }}
          env:
            - name: CSI_ENDPOINT
            {{- if .Values.node.windowsHostProcess }}
              value: unix://C:\\var\\lib\\kubelet\\plugins\\ebs.csi.aws.com\\csi.sock
            {{- else }}
              value: unix:/csi/csi.sock
            {{- end }}
            - name: CSI_NODE_NAME
              valueFrom:
                fieldRef:
                  fieldPath: spec.nodeName
            {{- if .Values.proxy.http_proxy }}
            {{- include "aws-ebs-csi-driver.http-proxy" . | nindent 12 }}
            {{- end }}
            {{- with .Values.node.otelTracing }}
            - name: OTEL_SERVICE_NAME
              value: {{ .otelServiceName }}
            - name: OTEL_EXPORTER_OTLP_ENDPOINT
              value: {{ .otelExporterEndpoint }}
            {{- end }}
            {{- with .Values.node.env }}
            {{- . | toYaml | nindent 12 }}
            {{- end }}
          volumeMounts:
            - name: kubelet-dir
              mountPath: C:\var\lib\kubelet
              mountPropagation: "None"
            - name: plugin-dir
              mountPath: C:\csi
          {{- if not .Values.node.windowsHostProcess }}
            - name: csi-proxy-disk-pipe
              mountPath: \\.\pipe\csi-proxy-disk-v1
            - name: csi-proxy-volume-pipe
              mountPath: \\.\pipe\csi-proxy-volume-v1
            - name: csi-proxy-filesystem-pipe
              mountPath: \\.\pipe\csi-proxy-filesystem-v1
          {{- end }}
          ports:
            - name: healthz
              containerPort: 9808
              protocol: TCP
          livenessProbe:
            httpGet:
              path: /healthz
              port: healthz
            initialDelaySeconds: 10
            timeoutSeconds: 3
            periodSeconds: 10
            failureThreshold: 5
          {{- with .Values.node.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
          {{- if not .Values.node.windowsHostProcess }}
          securityContext:
            windowsOptions:
              runAsUserName: "ContainerAdministrator"
          {{- end }}
          lifecycle:
            preStop:
              exec:
                command: ["/bin/aws-ebs-csi-driver", "pre-stop-hook"]
        - name: node-driver-registrar
          image: {{ printf "%s%s:%s" (default "" .Values.image.containerRegistry) .Values.sidecars.nodeDriverRegistrar.image.repository .Values.sidecars.nodeDriverRegistrar.image.tag }}
          imagePullPolicy: {{ default .Values.image.pullPolicy .Values.sidecars.nodeDriverRegistrar.image.pullPolicy }}
          {{- if .Values.node.windowsHostProcess }}
          command:
            - "csi-node-driver-registrar.exe"
          {{- end }}
          args:
            - --csi-address=$(ADDRESS)
            - --kubelet-registration-path=$(DRIVER_REG_SOCK_PATH)
          {{- if .Values.node.windowsHostProcess }}
            - --plugin-registration-path=$(PLUGIN_REG_DIR)
          {{- end }}
            - --v={{ .Values.sidecars.nodeDriverRegistrar.logLevel }}
          env:
            - name: ADDRESS
            {{- if .Values.node.windowsHostProcess }}
              value: unix://C:\\var\\lib\\kubelet\\plugins\\ebs.csi.aws.com\\csi.sock
            {{- else }}
              value: unix:/csi/csi.sock
            {{- end }}
            - name: DRIVER_REG_SOCK_PATH
            {{- if .Values.node.windowsHostProcess }}
              value: C:\\var\\lib\\kubelet\\plugins\\ebs.csi.aws.com\\csi.sock
            {{- else }}
              value: C:\var\lib\kubelet\plugins\ebs.csi.aws.com\csi.sock
            {{- end }}
          {{- if .Values.node.windowsHostProcess }}
            - name: PLUGIN_REG_DIR
              value: C:\\var\\lib\\kubelet\\plugins_registry\\
          {{- end }}
            {{- if .Values.proxy.http_proxy }}
            {{- include "aws-ebs-csi-driver.http-proxy" . | nindent 12 }}
            {{- end }}
            {{- with .Values.sidecars.nodeDriverRegistrar.env }}
            {{- . | toYaml | nindent 12 }}
            {{- end }}
          livenessProbe:
            exec:
              command:
                - /csi-node-driver-registrar.exe
                - --kubelet-registration-path=$(DRIVER_REG_SOCK_PATH)
                - --mode=kubelet-registration-probe
            initialDelaySeconds: 30
            timeoutSeconds: 15
            periodSeconds: 90
          volumeMounts:
            - name: plugin-dir
              mountPath: C:\csi
            - name: registration-dir
              mountPath: C:\registration
            - name: probe-dir
              mountPath: C:\var\lib\kubelet\plugins\ebs.csi.aws.com
          {{- with default .Values.node.resources .Values.sidecars.nodeDriverRegistrar.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
        - name: liveness-probe
          image: {{ printf "%s%s:%s" (default "" .Values.image.containerRegistry) .Values.sidecars.livenessProbe.image.repository .Values.sidecars.livenessProbe.image.tag }}
          imagePullPolicy: {{ default .Values.image.pullPolicy .Values.sidecars.livenessProbe.image.pullPolicy }}
          {{- if .Values.node.windowsHostProcess }}
          command:
            - "livenessprobe.exe"
          {{- end }}
          args:
            {{- if .Values.node.windowsHostProcess }}
            - --csi-address=unix://C:\\var\\lib\\kubelet\\plugins\\ebs.csi.aws.com\\csi.sock
            {{- else }}
            - --csi-address=unix:/csi/csi.sock
            {{- end }}
          volumeMounts:
            - name: plugin-dir
              mountPath: C:\csi
          {{- with default .Values.node.resources .Values.sidecars.livenessProbe.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- if .Values.imagePullSecrets }}
      imagePullSecrets:
      {{- range .Values.imagePullSecrets }}
        - name: {{ . }}
      {{- end }}
      {{- end }}
      volumes:
        - name: kubelet-dir
          hostPath:
            path: C:\var\lib\kubelet
            type: Directory
        - name: plugin-dir
          hostPath:
            path: C:\var\lib\kubelet\plugins\ebs.csi.aws.com
            type: DirectoryOrCreate
        - name: registration-dir
          hostPath:
            path: C:\var\lib\kubelet\plugins_registry
            type: Directory
      {{- if not .Values.node.windowsHostProcess }}
        - name: csi-proxy-disk-pipe
          hostPath:
            path: \\.\pipe\csi-proxy-disk-v1
            type: ""
        - name: csi-proxy-volume-pipe
          hostPath:
            path: \\.\pipe\csi-proxy-volume-v1
            type: ""
        - name: csi-proxy-filesystem-pipe
          hostPath:
            path: \\.\pipe\csi-proxy-filesystem-v1
            type: ""
      {{- end }}
        - name: probe-dir
          {{- if .Values.node.probeDirVolume }}
          {{- toYaml .Values.node.probeDirVolume | nindent 10 }}
          {{- else }}
          emptyDir: {}
          {{- end }}
{{- end }}
{{- end }}
