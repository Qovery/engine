data "external" "get_engine_version_to_use" {
  program = ["./helper.sh", "get_engine_version_to_use", var.qovery_engine_info.token, var.qovery_engine_info.api_fqdn, var.kubernetes_cluster_id]
}

resource "helm_release" "qovery_engine_resources" {
  name = "qovery-engine"
  chart = "common/charts/qovery-engine"
  namespace = "qovery"
  atomic = true
  create_namespace = true
  max_history = 50
  force_update = true

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "volumes.storageClassName"
    value = "aws-ebs-gp2-0"
  }

  set {
    name = "image.tag"
    value = data.external.get_engine_version_to_use.result.version
  }

  set {
    name = "environmentVariables.NATS_SERVER"
    value = var.qovery_nats_url
  }

  set {
    name = "environmentVariables.ORGANIZATION"
    value = var.organization_id
  }

  set {
    name = "environmentVariables.CLOUD_PROVIDER"
    value = var.cloud_provider
  }

  set {
    name = "environmentVariables.REGION"
    value = var.region
  }

  set {
    name = "environmentVariables.LIB_ROOT_DIR"
    value = "/home/qovery/lib"
  }

  set {
    name = "environmentVariables.DOCKER_HOST"
    value = "tcp://0.0.0.0:2375"
  }

  # Engine Limits
  set {
    name = "engineResources.limits.cpu"
    value = "1"
  }

  set {
    name = "engineResources.requests.cpu"
    value = "500m"
  }

  set {
    name = "engineResources.limits.memory"
    value = "512Mi"
  }

  set {
    name = "engineResources.requests.memory"
    value = "512Mi"
  }

  # Build limits
  set {
    name = "buildResources.limits.cpu"
    value = "1"
  }

  set {
    name = "buildResources.requests.cpu"
    value = "500m"
  }

  set {
    name = "buildResources.limits.memory"
    value = "4Gi"
  }

  set {
    name = "buildResources.requests.memory"
    value = "4Gi"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
    helm_release.cluster_autoscaler,
  ]
}
