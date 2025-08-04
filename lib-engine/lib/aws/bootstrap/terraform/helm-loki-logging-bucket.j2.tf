# This file is used to create the s3 bucket for the s3 access logs
{%- if object_storage_enable_logging %}

resource "aws_iam_role" "iam_eks_loki_logs" {
  name        = "qovery-logs-${var.kubernetes_cluster_id}-access-logs"
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
            "Sid": "accessLogsS3Loki",
            "Effect": "Allow",
            "Action": [
                "s3:Get*",
                "s3:List*",
                "s3:PutObject",
                "s3:DeleteObject"
            ],
            "Resource": [
                "${aws_s3_bucket.loki_bucket_logs.arn}",
                "${aws_s3_bucket.loki_bucket_logs.arn}/*"
            ]
        }
    ]
}
POLICY
}

resource "aws_iam_role_policy_attachment" "s3_loki_attachment_logs" {
  role       = aws_iam_role.iam_eks_loki_logs.name
  policy_arn = aws_iam_policy.loki_s3_policy_logs.arn
}

// S3 bucket to store indexes and logs
resource "aws_s3_bucket_versioning" "loki_bucket_versioning_logs" {
  bucket = aws_s3_bucket.loki_bucket_logs.id
  versioning_configuration {
    status = "Enabled"
  }
  lifecycle {
    ignore_changes = [
        versioning_configuration[0].mfa_delete
    ]
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
  acl    = "log-delivery-write"

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

  # This rule removes non-current versions after 3 days, deletes objects after loki_log_retention_in_week*7 days, and aborts incomplete multipart uploads after 1 day
  rule {
    id = "CleanLokiAuditLogVersions"
    status = "Enabled"

    noncurrent_version_expiration {
      noncurrent_days = 3
    }

    expiration {
      days = var.loki_log_retention_in_week * 7
    }

    abort_incomplete_multipart_upload {
      days_after_initiation = 1
    }
  }
}

resource "aws_s3_bucket_logging" "loki_bucket_logging" {
  bucket = aws_s3_bucket.loki_bucket.id
  target_bucket = aws_s3_bucket.loki_bucket_logs.id
  target_prefix = "logs/"
}

resource "aws_s3_bucket_policy" "loki_bucket_logs_bucket_policy" {
  bucket = aws_s3_bucket.loki_bucket_logs.id
  policy = <<POLICY
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Principal": {
                "Service": "logging.s3.amazonaws.com"
            },
            "Action": "s3:PutObject",
            "Resource": "arn:aws:s3:::${aws_s3_bucket.loki_bucket_logs.id}/*",
            "Condition": {
                "StringEquals": {
                    "aws:SourceAccount": "${data.aws_caller_identity.current.account_id}"
                }
            }
        }
    ]
}
POLICY
}
{%- endif %}