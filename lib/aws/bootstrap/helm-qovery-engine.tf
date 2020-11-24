data "external" "get_engine_version_to_use" {
  program = ["./helper.sh", "get_engine_version_to_use", var.qovery_engine_info.token, var.qovery_engine_info.api_fqdn, var.eks_cluster_id]
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
    name = "image.tag"
    value = data.external.get_engine_version_to_use.result.version
  }

  set {
    name = "environmentVariables.ENGINE_RES_URL"
    value = "https://prod-qengine-resources.s3.eu-west-3.amazonaws.com/${data.external.get_engine_version_to_use.result.version}-lib.tgz"
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

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "1"
  }

  set {
    name = "resources.requests.cpu"
    value = "500m"
  }

  set {
    name = "resources.limits.memory"
    value = "4Gi"
  }

  set {
    name = "resources.requests.memory"
    value = "4Gi"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}
