resource "aws_iam_user" "iam_eks_loki" {
  name = "qovery-logs-${var.eks_cluster_id}"
}

resource "aws_iam_access_key" "iam_eks_loki" {
  user    = aws_iam_user.iam_eks_loki.name
}

resource "aws_iam_policy" "loki_s3_policy" {
  name = aws_iam_user.iam_eks_loki.name
  description = "Policy for logs storage"

  policy = <<POLICY
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": "s3:*",
            "Resource": "*"
        }
    ]
}
POLICY
}

resource "aws_iam_user_policy_attachment" "s3_loki_attachment" {
  user       = aws_iam_user.iam_eks_loki.name
  policy_arn = aws_iam_policy.loki_s3_policy.arn
}

// S3 bucket to store indexes and logs
resource "aws_s3_bucket" "loki_bucket" {
  bucket = aws_iam_user.iam_eks_loki.name
  acl    = "private"
  versioning {
    enabled = false
  }

  tags = merge(
    local.tags_eks,
    {
      "Name" = "Applications logs"
    }
  )
}

resource "helm_release" "loki" {
  name = "loki"
  chart = "common/charts/loki"
  namespace = "logging"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/loki.yaml")]

  set {
    name = "config.storage_config.aws.s3"
    value = "s3://${urlencode(aws_iam_access_key.iam_eks_loki.id)}:${urlencode(aws_iam_access_key.iam_eks_loki.secret)}@${var.region}/${aws_s3_bucket.loki_bucket.bucket}"
  }

  set {
    name = "config.storage_config.aws.region"
    value = var.region
  }

  set {
    name = "config.storage_config.aws.access_key_id"
    value = aws_iam_access_key.iam_eks_loki.id
  }

  set {
    name = "config.storage_config.aws.secret_access_key"
    value = aws_iam_access_key.iam_eks_loki.secret
  }

  set {
    name = "priorityClassName"
    value = "high-priority"
  }

  depends_on = [
    aws_iam_user.iam_eks_loki,
    aws_iam_access_key.iam_eks_loki,
    aws_s3_bucket.loki_bucket,
    aws_iam_policy.loki_s3_policy,
    aws_iam_user_policy_attachment.s3_loki_attachment,
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}
