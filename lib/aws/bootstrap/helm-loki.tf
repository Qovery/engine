resource "aws_iam_user" "iam_eks_loki" {
  name = "qovery-logs-${var.kubernetes_cluster_id}"
  tags = local.tags_eks
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

resource "aws_kms_key" "s3_logs_kms_encryption" {
  description             = "s3 logs encryption"
  tags = merge(
    local.tags_eks,
    {
      "Name" = "Encryption logs"
    }
  )
}

// S3 bucket to store indexes and logs
resource "aws_s3_bucket" "loki_bucket" {
  bucket = aws_iam_user.iam_eks_loki.name
  acl    = "private"
  force_destroy = true
  versioning {
    enabled = false
  }

  server_side_encryption_configuration {
    rule {
      apply_server_side_encryption_by_default {
        kms_master_key_id = aws_kms_key.s3_logs_kms_encryption.arn
        sse_algorithm = "aws:kms"
      }
    }
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
    name = "config.storage_config.aws.sse_encryption"
    value = "true"
  }

  # Limits
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
    value = "2Gi"
  }

  set {
    name = "resources.requests.memory"
    value = "1Gi"
  }

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    aws_iam_user.iam_eks_loki,
    aws_iam_access_key.iam_eks_loki,
    aws_s3_bucket.loki_bucket,
    aws_iam_policy.loki_s3_policy,
    aws_iam_user_policy_attachment.s3_loki_attachment,
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
    helm_release.cluster_autoscaler,
  ]
}
