resource "aws_iam_user" "fluent-bit" {
  name = "${var.eks_cluster_id}-fluent-bit"

  tags = aws_eks_cluster.eks_cluster.tags
}

resource "aws_iam_access_key" "fluent-bit" {
  user    = aws_iam_user.fluent-bit.name

  tags = aws_eks_cluster.eks_cluster.tags
}

resource "helm_release" "fluent-bit" {
  name = "fluent-bit"
  chart = "../../../lib/common/charts/fluent-bit"
  namespace = "logging"
  create_namespace = "true"
  atomic = true
  max_history = 50
  force_update = true

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
    name = "awsEsProxy.host"
    value = aws_elasticsearch_domain.qovery_eks_logs.endpoint
  }

  set {
    name = "awsEsProxy.port"
    value = "9200"
  }

  set {
    name = "awsEsProxy.accessKey"
    value = aws_iam_access_key.fluent-bit.id
  }

  set {
    name = "awsEsProxy.secretKey"
    value = aws_iam_access_key.fluent-bit.secret
  }

  set {
    name = "backend.type"
    value = "es"
  }

  set {
    name = "backend.es.host"
    value = "localhost"
  }

  // AWS direct authentication is not production ready
//  set {
//    name = "image.fluent_bit.tag"
//    value = "1.4.6"
//  }
//  set {
//    name = "backend.es.host"
//    value = aws_elasticsearch_domain.qovery-k8s-logs.endpoint
//  }
//
//  set {
//    name = "backend.es.port"
//    value = "443"
//  }
//
//  set {
//    name = "backend.es.aws_auth"
//    value = "on"
//  }
//
//  set {
//    name = "backend.es.aws_region"
//    value = var.region
//  }
//
//  set {
//    name = "backend.es.tls"
//    value = "on"
//  }
//
//  set {
//    name = "env[0].name"
//    value = "AWS_ACCESS_KEY_ID"
//  }
//  set {
//    name = "env[0].value"
//    value = aws_iam_access_key.fluent-bit.id
//  }
//
//  set {
//    name = "env[1].name"
//    value = "AWS_SECRET_ACCESS_KEY"
//  }
//  set {
//    name = "env[1].value"
//    value = aws_iam_access_key.fluent-bit.secret
//  }
//
//  set {
//    name = "env[2].name"
//    value = "AWS_SESSION_TOKEN"
//  }
//  set {
//    name = "env[2].value"
//    value = aws_iam_access_key.fluent-bit.secret
//  }

  depends_on = [aws_eks_cluster.eks_cluster, helm_release.prometheus-operator]
}