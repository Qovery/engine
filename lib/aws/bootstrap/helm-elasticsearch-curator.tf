resource "helm_release" "elasticsearch_curator" {
  name = "elasticsearch-curator"
  chart = "common/charts/elasticsearch-curator"
  namespace = "logging"
  create_namespace = "true"
  atomic = true
  max_history = 50

  values = [file("chart_values/elasticsearch-curator.yaml")]

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "image.repository"
    value = "qoveryrd/curator"
  }

  set {
    name = "priorityClassName"
    value = "medium-priority"
  }

  set {
    name = "image.tag"
    value = "5.8.1-aws"
  }

  set {
    name = "es_endpoint"
    value = aws_elasticsearch_domain.qovery_eks_logs.endpoint
  }

  set {
    name = "cronjob.schedule"
    value = "0 1 * * *"
  }

  set {
    name = "cronjob.failedJobsHistoryLimit"
    value = "3"
  }

  set {
    name = "cronjob.successfulJobsHistoryLimit"
    value = "1"
  }

  set {
    name = "rbac.enabled"
    value = "true"
  }

  set {
    name = "psp.create"
    value = "true"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    aws_elasticsearch_domain.qovery_eks_logs,
    helm_release.aws_vpc_cni,
  ]
}