{%- if aws_enable_vpc_flow_logs -%}
// S3 bucket for VPC flow logs
resource "aws_s3_bucket" "vpc_flow_logs" {
  bucket = var.s3_flow_logs_bucket_name
  force_destroy = true

  tags = merge(
    local.tags_eks,
    {
      "Name" = "VPC flow logs"
    }
  )
}

resource "aws_s3_bucket_lifecycle_configuration" "vpc_flow_logs_lifecycle" {
  bucket = aws_s3_bucket.vpc_flow_logs.id
  rule {
    id = "on_delete_rule"

    filter {
      prefix = ""
    }

    expiration {
      days = var.vpc_flow_logs_retention_days
    }

    noncurrent_version_expiration {
      noncurrent_days = 1
    }

    {%- if vpc_flow_logs_retention_days > 0 %}
    status = "Enabled"
    {%- else %}
    status = "Disabled"
    {%- endif %}
  }

}

resource "aws_s3_bucket_versioning" "vpc_flow_logs_versionning" {
  bucket = aws_s3_bucket.vpc_flow_logs.id
  versioning_configuration {
    status = "Disabled"
  }
}

resource "aws_s3_bucket_acl" "vpc_flow_logs_acl" {
  bucket = aws_s3_bucket.vpc_flow_logs.id
  acl    = "private"

  depends_on = [
    aws_s3_bucket_ownership_controls.vpc_flow_logs_bucket_ownership,
    aws_s3_bucket_public_access_block.flow_logs_access,
  ]
}

resource "aws_s3_bucket_ownership_controls" "vpc_flow_logs_bucket_ownership" {
  bucket = aws_s3_bucket.vpc_flow_logs.id
  rule {
    object_ownership = "ObjectWriter"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "flow_logs_bucket_encryption" {
  bucket = aws_s3_bucket.vpc_flow_logs.id

  rule {
    blocked_encryption_types = ["SSE-C"]
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.s3_logs_kms_encryption.arn
      sse_algorithm = "aws:kms"
    }
    bucket_key_enabled = true
  }
}

resource "aws_s3_bucket_public_access_block" "flow_logs_access" {
  bucket = aws_s3_bucket.vpc_flow_logs.id

  ignore_public_acls = true
  restrict_public_buckets  = true
  block_public_policy = true
  block_public_acls = true
}

# The delivery Allow statements mirror the policy AWS auto-attaches for VPC
# flow log delivery to S3. We manage them in Terraform so the Deny statement
# coexists with delivery: when the flow log is created AWS overwrites the bucket
# policy with its delivery-only version, so this resource depends_on the flow log
# to be re-applied last and restore the full policy (delivery + HTTPS deny).
resource "aws_s3_bucket_policy" "vpc_flow_logs_bucket_policy" {
  bucket = aws_s3_bucket.vpc_flow_logs.id
  policy = <<POLICY
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "AWSLogDeliveryWrite",
            "Effect": "Allow",
            "Principal": {
                "Service": "delivery.logs.amazonaws.com"
            },
            "Action": "s3:PutObject",
            "Resource": "${aws_s3_bucket.vpc_flow_logs.arn}/*",
            "Condition": {
                "StringEquals": {
                    "s3:x-amz-acl": "bucket-owner-full-control",
                    "aws:SourceAccount": "${data.aws_caller_identity.current.account_id}"
                },
                "ArnLike": {
                    "aws:SourceArn": "arn:aws:logs:${var.region}:${data.aws_caller_identity.current.account_id}:*"
                }
            }
        },
        {
            "Sid": "AWSLogDeliveryAclCheck",
            "Effect": "Allow",
            "Principal": {
                "Service": "delivery.logs.amazonaws.com"
            },
            "Action": "s3:GetBucketAcl",
            "Resource": "${aws_s3_bucket.vpc_flow_logs.arn}",
            "Condition": {
                "StringEquals": {
                    "aws:SourceAccount": "${data.aws_caller_identity.current.account_id}"
                },
                "ArnLike": {
                    "aws:SourceArn": "arn:aws:logs:${var.region}:${data.aws_caller_identity.current.account_id}:*"
                }
            }
        },
        {
            "Sid": "DenyInsecureTransport",
            "Effect": "Deny",
            "Principal": "*",
            "Action": "s3:*",
            "Resource": [
                "${aws_s3_bucket.vpc_flow_logs.arn}",
                "${aws_s3_bucket.vpc_flow_logs.arn}/*"
            ],
            "Condition": {
                "Bool": {
                    "aws:SecureTransport": "false"
                }
            }
        }
    ]
}
POLICY
{%- if not user_provided_network %}

  depends_on = [aws_flow_log.eks_vpc_flow_logs]
{%- endif %}
}
{%- endif -%}
