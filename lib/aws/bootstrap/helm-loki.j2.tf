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

resource "aws_s3_bucket_public_access_block" "loki_access" {
  bucket = aws_s3_bucket.loki_bucket.id

  ignore_public_acls = true
  restrict_public_buckets  = true
  block_public_policy = true
  block_public_acls = true
}