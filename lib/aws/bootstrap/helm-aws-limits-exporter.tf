resource "aws_iam_user" "iam_aws_limits_exporter" {
  name = "qovery-aws-limits-exporter-${var.kubernetes_cluster_id}"

  tags = local.tags_eks
}

resource "aws_iam_access_key" "iam_aws_limits_exporter" {
  user    = aws_iam_user.iam_aws_limits_exporter.name
}

resource "aws_iam_user_policy" "iam_aws_limits_exporter" {
  name = aws_iam_user.iam_aws_limits_exporter.name
  user = aws_iam_user.iam_aws_limits_exporter.name

  policy = <<EOF
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "support:*"
            ],
            "Resource": [
                "*"
            ]
        }
    ]
}
EOF
}

resource "helm_release" "iam_aws_limits_exporter" {
  name = "aws-limits-exporter"
  chart = "charts/aws-limits-exporter"
  namespace = "prometheus"
  create_namespace = true
  atomic = true
  max_history = 50

  // We can't activate it now until we got the support info into metadata field
  // make a fake arg to avoid TF to validate update on failure because of the atomic option
//  set {
//    name = "fake"
//    value = timestamp()
//  }

  set {
    name = "awsCredentials.awsAccessKey"
    value = aws_iam_access_key.iam_aws_limits_exporter.id
  }

  set {
    name = "awsCredentials.awsSecretKey"
    value = aws_iam_access_key.iam_aws_limits_exporter.secret
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.cluster_autoscaler,
    helm_release.aws_vpc_cni,
  ]
}
