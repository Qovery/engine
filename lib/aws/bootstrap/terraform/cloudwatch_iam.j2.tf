{% if enable_cloudwatch_exporter -%}

data "aws_iam_policy_document" "cloudwatch_exporter_policy" {
  statement {
    sid    = "CloudWatchReadAccess"
    effect = "Allow"
    actions = [
      "cloudwatch:GetMetricData",
      "cloudwatch:GetMetricStatistics",
      "cloudwatch:ListMetrics",
    ]
    resources = ["*"]
  }

  statement {
    sid    = "EC2ReadAccess"
    effect = "Allow"
    actions = [
      "ec2:DescribeInstances",
      "ec2:DescribeRegions",
      "ec2:DescribeVolumes",
    ]
    resources = ["*"]
  }

  statement {
    sid    = "RDSReadAccess"
    effect = "Allow"
    actions = [
      "rds:DescribeDBInstances",
      "rds:DescribeDBClusters",
      "rds:ListTagsForResource",
    ]
    resources = ["*"]
  }

  statement {
    sid    = "TagReadAccess"
    effect = "Allow"
    actions = [
      "tag:GetResources",
    ]
    resources = ["*"]
  }

  statement {
    sid    = "IAMReadAccess"
    effect = "Allow"
    actions = [
     "iam:ListAccountAliases",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_policy" "cloudwatch_exporter" {
  name        = "qovery-cloudwatch-exporter-policy-${var.kubernetes_cluster_id}"
  description = "Read-only access to CloudWatch metrics for Prometheus export"
  policy      = data.aws_iam_policy_document.cloudwatch_exporter_policy.json
}

resource "aws_iam_role" "cloudwatch_exporter" {
  name        = "qovery-cloudwatch-exporter-${var.kubernetes_cluster_id}"
  description = "CloudWatch Exporter role for EKS cluster ${var.kubernetes_cluster_id}"
  tags        = local.tags_eks

  assume_role_policy = jsonencode({
    "Version": "2012-10-17",
    "Statement": [
      {
        "Effect": "Allow",
        "Principal": {
          "Federated": "${aws_iam_openid_connect_provider.oidc.arn}"
        },
        "Action": "sts:AssumeRoleWithWebIdentity",
        "Condition": {
          "StringEquals": {
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub": "system:serviceaccount:qovery:prometheus-yet-another-cloudwatch-exporter",
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:aud": "sts.amazonaws.com"
          }
        }
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "cloudwatch_exporter" {
  role       = aws_iam_role.cloudwatch_exporter.name
  policy_arn = aws_iam_policy.cloudwatch_exporter.arn
}

{% endif -%}
