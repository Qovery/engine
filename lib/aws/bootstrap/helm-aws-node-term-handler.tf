resource "helm_release" "aws_node_term_handler" {
  name = "aws-node-term-handler"
  chart = "charts/aws-node-termination-handler"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "nameOverride"
    value = "aws-node-term-handler"
  }

  set {
    name = "fullnameOverride"
    value = "aws-node-term-handler"
  }

  set {
    name = "image.tag"
    value = "v1.5.0"
  }

  set {
    name = "enableSpotInterruptionDraining"
    value = "true"
  }

  set {
    name = "enableScheduledEventDraining"
    value = "true"
  }

  set {
    name = "deleteLocalData"
    value = "true"
  }

  set {
    name = "ignoreDaemonSets"
    value = "true"
  }

  set {
    name = "podTerminationGracePeriod"
    value = "300"
  }

  set {
    name = "nodeTerminationGracePeriod"
    value = "120"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
    helm_release.cluster_autoscaler,
  ]
}