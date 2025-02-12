{%- if object_storage_enable_logging %}

resource "aws_iam_role" "iam_eks_loki_logs" {
  name        = "qovery-logs-${var.kubernetes_cluster_id}-log"
  tags        = local.tags_eks

  assume_role_policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "${aws_iam_openid_connect_provider.oidc.arn}"
      },
      "Action": ["sts:AssumeRoleWithWebIdentity"],
      "Condition": {
        "StringEquals": {
          "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub": "system:serviceaccount:logging:loki"
        }
      }
    }
  ]
}
POLICY
}

resource "aws_iam_policy" "loki_s3_policy_logs" {
  name = aws_iam_role.iam_eks_loki_logs.name
  description = "Policy for logs storage"

  policy = <<POLICY
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "s3:*",
                "kms:*"
            ],
            "Resource": "*"
        }
    ]
}
POLICY
}

resource "aws_iam_role_policy_attachment" "s3_loki_attachment_logs" {
  role       = aws_iam_role.iam_eks_loki_logs.name
  policy_arn = aws_iam_policy.loki_s3_policy_logs.arn
}

resource "aws_kms_key" "s3_logs_kms_encryption_logs" {
  description             = "s3 logs encryption"
  enable_key_rotation     = true
  tags = merge(
    local.tags_eks,
    {
      "Name" = "Encryption logs"
    }
  )
}

resource "aws_s3_bucket_server_side_encryption_configuration" "lok_bucket_enryption_logs" {
  bucket = aws_s3_bucket.loki_bucket_logs.id

  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.s3_logs_kms_encryption_logs.arn
      sse_algorithm = "aws:kms"
    }
  }
}

resource "aws_s3_bucket_ownership_controls" "loki_bucket_ownership_logs" {
  bucket = aws_s3_bucket.loki_bucket_logs.id
  rule {
    object_ownership = "ObjectWriter"
  }
}

resource "aws_s3_bucket_acl" "loki_bucket_acl_logs" {
  bucket = aws_s3_bucket.loki_bucket_logs.id
  acl    = "private"

  depends_on = [
    aws_s3_bucket_ownership_controls.loki_bucket_ownership_logs,
    aws_s3_bucket_public_access_block.loki_access_logs,
  ]
}

resource "aws_s3_bucket_public_access_block" "loki_access_logs" {
  bucket = aws_s3_bucket.loki_bucket_logs.id

  ignore_public_acls = true
  restrict_public_buckets  = true
  block_public_policy = true
  block_public_acls = true
}

resource "aws_s3_bucket" "loki_bucket_logs" {
  bucket = aws_iam_role.iam_eks_loki_logs.name
  force_destroy = true

  tags = merge(
    local.tags_eks,
    {
    {% if is_deletion_step %}
    "can_be_deleted_by_owner" = "true"
    {% endif %}
    "Name" = "Applications logs"
    }
  )
}

resource "aws_s3_bucket_lifecycle_configuration" "loki_lifecycle_logs" {
  bucket = aws_s3_bucket.loki_bucket_logs.id
  rule {
    id = "on_delete_rule"

    expiration {
      days = 1
    }

    noncurrent_version_expiration {
      noncurrent_days = 1
    }

  {% if is_deletion_step %}
  status = "Enabled"
  {% else %}
  status = "Disabled"
  {% endif %}
  }

}

resource "aws_s3_bucket_logging" "loki_bucket_logging" {
  bucket = aws_s3_bucket.loki_bucket.id
  target_bucket = aws_s3_bucket.loki_bucket_logs.id
  target_prefix = "logs/"
}
{%- endif %}
