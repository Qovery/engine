resource "helm_release" "pleco" {
  count = var.test_cluster == "false" ? 0 : 1

  name = "pleco"
  chart = "common/charts/pleco"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "enabledFeatures.rds"
    value = "true"
  }

  set {
    name = "enabledFeatures.documentdb"
    value = "true"
  }

  set {
    name = "enabledFeatures.elasticache"
    value = "true"
  }

  set {
    name = "enabledFeatures.disableDryRun"
    value = "true"
  }

  set {
    name = "environmentVariables.AWS_ACCESS_KEY_ID"
    value = "{{ aws_access_key }}"
  }

  set {
    name = "environmentVariables.AWS_SECRET_ACCESS_KEY"
    value = "{{ aws_secret_key }}"
  }

  set {
    name = "environmentVariables.AWS_DEFAULT_REGION"
    value = "{{ aws_region }}"
  }

  set {
    name = "environmentVariables.LOG_LEVEL"
    value = "debug"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
    helm_release.cluster_autoscaler,
  ]
}