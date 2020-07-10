resource "aws_iam_user" "fluent-bit" {
  name = "fluent-bit-${var.region_cluster_name}"
}

resource "aws_iam_access_key" "fluent-bit" {
  user    = aws_iam_user.fluent-bit.name
}

resource "helm_release" "fluent-bit" {
  name = "fluent-bit"
  chart = "../../../lib/common/charts/fluent-bit"
  namespace = "prometheus"
  atomic = true
  max_history = 50

  values = [file("chart_values/fluent-bit.yaml")]

  # Override image to support AWS auth
  set {
    name = "image.fluent_bit.tag"
    value = "1.4.6"
  }

  # Enable Prometheus exporter
  set {
    name = "metrics.enabled"
    value = "true"
  }

  set {
    name = "metrics.serviceMonitor.enabled"
    value = "true"
  }

  # Set AWS auth
  set {
    name = "backend.es.host"
    value = aws_elasticsearch_domain.qovery-k8s-logs.endpoint
  }

  set {
    name = "backend.es.port"
    value = "443"
  }

  set {
    name = "backend.es.aws_auth"
    value = "On"
  }

  set {
    name = "backend.es.aws_region"
    value = var.region
  }

  set {
    name = "backend.es.tls"
    value = "On"
  }

  set {
    name = "AWS_ACCESS_KEY_ID"
    value = aws_iam_access_key.fluent-bit.id
  }

  set {
    name = "AWS_SECRET_ACCESS_KEY"
    value = aws_iam_access_key.fluent-bit.secret
  }

  depends_on = [aws_eks_cluster.eks_cluster, helm_release.prometheus-operator]
}