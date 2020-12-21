resource "aws_iam_user" "iam_eks_cluster_autoscaler" {
  name = "qovery-clustauto-${var.eks_cluster_id}"
}

resource "aws_iam_access_key" "iam_eks_cluster_autoscaler" {
  user    = aws_iam_user.iam_eks_cluster_autoscaler.name
}

resource "aws_iam_policy" "cluster_autoscaler_policy" {
  name = aws_iam_user.iam_eks_cluster_autoscaler.name
  description = "Policy for cluster autoscaler"

  policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "autoscaling:DescribeAutoScalingGroups",
        "autoscaling:DescribeAutoScalingInstances",
        "autoscaling:DescribeLaunchConfigurations",
        "autoscaling:DescribeTags",
        "autoscaling:SetDesiredCapacity",
        "autoscaling:TerminateInstanceInAutoScalingGroup"
      ],
      "Resource": "*"
    }
  ]
}
POLICY
}

resource "aws_iam_user_policy_attachment" "s3_cluster_autoscaler_attachment" {
  user       = aws_iam_user.iam_eks_cluster_autoscaler.name
  policy_arn = aws_iam_policy.cluster_autoscaler_policy.arn
}

resource "helm_release" "cluster_autoscaler" {
  name = "cluster-autoscaler"
  chart = "common/charts/cluster-autoscaler"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "cloudProvider"
    value = "aws"
  }

  set {
    name = "autoDiscovery.clusterName"
    value = var.eks_cluster_name
  }

  set {
    name = "awsRegion"
    value = var.region
  }

  set {
    name = "awsAccessKeyID"
    value = aws_iam_access_key.iam_eks_cluster_autoscaler.id
  }

  set {
    name = "awsSecretAccessKey"
    value = aws_iam_access_key.iam_eks_cluster_autoscaler.secret
  }

  set {
    name = "priorityClassName"
    value = "system-cluster-critical"
  }

  # observability
  set {
    name = "serviceMonitor.enabled"
    value = "true"
  }

  set {
    name = "serviceMonitor.namespace"
    value = local.prometheus_namespace
  }

  # resources limitation
  set {
    name = "resources.limits.cpu"
    value = "100m"
  }

  set {
    name = "resources.requests.cpu"
    value = "100m"
  }

  set {
    name = "resources.limits.memory"
    value = "300Mi"
  }

  set {
    name = "resources.requests.memory"
    value = "300Mi"
  }

  depends_on = [
    aws_iam_user.iam_eks_cluster_autoscaler,
    aws_iam_access_key.iam_eks_cluster_autoscaler,
    aws_iam_user_policy_attachment.s3_cluster_autoscaler_attachment,
    aws_eks_cluster.eks_cluster,
    helm_release.prometheus_operator,
    helm_release.aws_vpc_cni,
  ]
}