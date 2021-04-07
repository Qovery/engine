locals {
  pleco_config = <<PLECO
enabledFeatures:
  disableDryRun: true
  checkInterval: 120
  kubernetes: "in"
  awsRegions:
    - eu-west-3
    - us-east-2
  rds: true
  documentdb: true
  elasticache: true
  eks: true
  elb: true
  ebs: true
  vpc: false
  s3: true
  kms: true
  cloudwatchLogs: true
  iam: true
  sshKeys: true
  ecr: true
PLECO
}

resource "helm_release" "pleco" {
  count = var.test_cluster == "false" ? 0 : 1

  name = "pleco"
  chart = "common/charts/pleco"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  values = [local.pleco_config]

  set {
    name = "environmentVariables.AWS_ACCESS_KEY_ID"
    value = "{{ aws_access_key }}"
  }

  set {
    name = "environmentVariables.AWS_SECRET_ACCESS_KEY"
    value = "{{ aws_secret_key }}"
  }

  set {
    name = "environmentVariables.LOG_LEVEL"
    value = "debug"
  }

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
    helm_release.cluster_autoscaler,
    helm_release.prometheus_operator,
  ]
}