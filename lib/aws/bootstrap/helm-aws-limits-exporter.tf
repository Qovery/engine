resource "aws_iam_user" "iam-aws-limits-exporter" {
  name = "aws-limits-exporter-${var.region_cluster_name}"
}

resource "aws_iam_access_key" "iam-aws-limits-exporter" {
  user    = aws_iam_user.iam-aws-limits-exporter.name
}

resource "aws_iam_user_policy" "iam-aws-limits-exporter" {
  name = "aws-limits-exporter-${var.region_cluster_name}"
  user = aws_iam_user.iam-aws-limits-exporter.name

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

resource "helm_release" "aws-limits-exporter" {
  name = "aws-limits-exporter"
  chart = "../../../lib/aws/charts/aws-limits-exporter"
  namespace = "prometheus"
  create_namespace = true
  atomic = true
  max_history = 50

  set {
    name = "awsCredentials.awsAccessKey"
    value = aws_iam_access_key.iam-aws-limits-exporter.id
  }

  set {
    name = "awsCredentials.awsSecretKey"
    value = aws_iam_access_key.iam-aws-limits-exporter.secret
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.prometheus-operator
  ]
}