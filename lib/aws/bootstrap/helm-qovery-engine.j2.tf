locals {
  qovery_engine_version = "1f7a0fe"
}

resource "helm_release" "qovery_engine_resources" {
  name = "qovery-engine"
  chart = "common/charts/qovery-engine"
  namespace = "qovery"
  atomic = true
  create_namespace = true
  max_history = 50

  set {
    name = "image.tag"
    value = local.qovery_engine_version
  }

  set {
    name = "environmentVariables.ENGINE_RES_URL"
    value = "s3://prod-qengine-resources/${local.qovery_engine_version}-lib.tgz"
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
    aws_eks_cluster.eks_cluster,
    aws_iam_access_key.qovery_engine_resources
  ]
}