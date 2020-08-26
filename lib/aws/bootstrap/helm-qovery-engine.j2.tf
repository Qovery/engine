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
    value = "nats://panic.qovery.com:4242"
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
    name = "environmentVariables.RUST_LOG"
    value = "info"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}