locals {
  qovery_engine_version = "eb7b237d8b7130cfdfdfb949fe15fba939373b76"
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
    value = local.qovery_engine_version
  }

  set {
    name = "environmentVariables.ENGINE_RES_URL"
    value = "https://prod-qengine-resources.s3.eu-west-3.amazonaws.com/${local.qovery_engine_version}-lib.tgz"
  }

  set {
    name = "environmentVariables.NATS_SERVER"
    value = "nats://panic.qovery.com:4242"
  }

  set {
    name = "environmentVariables.RUST_LOG"
    value = "info"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster
  ]
}