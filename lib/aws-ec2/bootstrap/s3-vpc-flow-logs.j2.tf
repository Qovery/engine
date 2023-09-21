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
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
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
{%- endif -%}