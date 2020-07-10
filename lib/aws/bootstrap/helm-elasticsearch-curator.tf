resource "helm_release" "elasticsearch-curator" {
  name = "elasticsearch-curator"
  chart = "../../../lib/common/charts/elasticsearch-curator"
  namespace = "prometheus"
  atomic = true
  max_history = 50

  values = [file("chart_values/elasticsearch-curator.yaml")]

  set {
    name = "image.repository"
    value = "qoveryrd/curator"
  }

  set {
    name = "image.tag"
    value = "5.8.1-aws"
  }

  set {
    name = "es_endpoint"
    value = aws_elasticsearch_domain.qovery-k8s-logs.endpoint
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
    aws_elasticsearch_domain.qovery-k8s-logs
  ]
}