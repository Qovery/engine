resource "aws_iam_role" "iam_eks_prometheus" {
  name        = "qovery-metrics-${var.kubernetes_cluster_id}"
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
          "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:aud": "sts.amazonaws.com"
        }
      }
    },
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "${aws_iam_openid_connect_provider.oidc.arn}"
      },
      "Action": ["sts:AssumeRoleWithWebIdentity"],
      "Condition": {
        "StringEquals": {
          "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub": "system:serviceaccount:prometheus:kube-prometheus-stack-prometheus"
        }
      }
    },
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "${aws_iam_openid_connect_provider.oidc.arn}"
      },
      "Action": ["sts:AssumeRoleWithWebIdentity"],
      "Condition": {
        "StringLike": {
          "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub": "system:serviceaccount:prometheus:thanos-*"
        }
      }
    }
  ]
}
POLICY
}

resource "aws_iam_policy" "prometheus_s3_policy" {
  name = aws_iam_role.iam_eks_prometheus.name
  description = "Policy for metrics storage"

  policy = <<POLICY
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "Statement",
            "Effect": "Allow",
            "Action": [
                "s3:ListBucket",
                "s3:GetObject",
                "s3:DeleteObject",
                "s3:PutObject"
            ],
            "Resource": [
                "arn:aws:s3:::${aws_s3_bucket.prometheus_bucket.id}/*",
                "arn:aws:s3:::${aws_s3_bucket.prometheus_bucket.id}"
            ]
        },
        {
            "Sid": "KMSAccess",
            "Effect": "Allow",
            "Action": [
                "kms:GenerateDataKey",
                "kms:Encrypt",
                "kms:Decrypt"
            ],
            "Resource": "${aws_kms_key.s3_metrics_kms_encryption.arn}"
        }
    ]
}
POLICY
}

resource "aws_iam_role_policy_attachment" "s3_prometheus_attachment" {
  role       = aws_iam_role.iam_eks_prometheus.name
  policy_arn = aws_iam_policy.prometheus_s3_policy.arn
}

resource "aws_kms_key" "s3_metrics_kms_encryption" {
  description             = "s3 metrics encryption"
  enable_key_rotation     = true
  tags = merge(
    local.tags_eks,
    {
      "Name" = "Encryption metrics"
    }
  )
}

resource "aws_s3_bucket_server_side_encryption_configuration" "prometheus_bucket_enryption" {
  bucket = aws_s3_bucket.prometheus_bucket.id

  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.s3_metrics_kms_encryption.arn
      sse_algorithm = "aws:kms"
    }
  }
}

// S3 bucket to store indexes and metrics
resource "aws_s3_bucket_versioning" "prometheus_bucket_versioning" {
  bucket = aws_s3_bucket.prometheus_bucket.id
  versioning_configuration {
    status = "Enabled"
  }
  lifecycle {
    ignore_changes = [
        versioning_configuration[0].mfa_delete
    ]
  }
}

resource "aws_s3_bucket_ownership_controls" "prometheus_bucket_ownership" {
  bucket = aws_s3_bucket.prometheus_bucket.id
  rule {
    object_ownership = "ObjectWriter"
  }
}

resource "aws_s3_bucket_acl" "prometheus_bucket_acl" {
  bucket = aws_s3_bucket.prometheus_bucket.id
  acl    = "private"

  depends_on = [
    aws_s3_bucket_ownership_controls.prometheus_bucket_ownership,
    aws_s3_bucket_public_access_block.prometheus_access,
  ]
}

resource "aws_s3_bucket_public_access_block" "prometheus_access" {
  bucket = aws_s3_bucket.prometheus_bucket.id

  ignore_public_acls = true
  restrict_public_buckets  = true
  block_public_policy = true
  block_public_acls = true
}

resource "aws_s3_bucket" "prometheus_bucket" {
  bucket = aws_iam_role.iam_eks_prometheus.name
  force_destroy = true

  tags = merge(
    local.tags_eks,
    {
    {% if is_deletion_step %}
    "can_be_deleted_by_owner" = "true"
    {% endif %}
    "Name" = "Applications metrics"
    }
  )
}

resource "aws_s3_bucket_lifecycle_configuration" "prometheus_lifecycle" {
  bucket = aws_s3_bucket.prometheus_bucket.id
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

  # This rule removes old versions of objects, cleans up delete markers, and aborts incomplete multipart uploads
  rule {
    id = "CleanThanosMetricsVersions"
    status = "Enabled"

    noncurrent_version_expiration {
      noncurrent_days = 3
    }

    expiration {
      expired_object_delete_marker = true
    }

    abort_incomplete_multipart_upload {
      days_after_initiation = 1
    }
  }
}

