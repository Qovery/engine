// S3 bucket to store kubeconfigs
resource "aws_s3_bucket" "kubeconfigs_bucket" {
  bucket = var.s3_bucket_kubeconfig
  force_destroy = true

  tags = merge(
    local.tags_eks,
    {
      "Name" = "Kubernetes kubeconfig"
    }
  )
}

resource "aws_s3_bucket_versioning" "kubeconfig_bucket_versioning" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_acl" "kubeconfig_bucket_acl" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id
  acl    = "private"
}

resource "aws_kms_key" "s3_kubeconfig_kms_encryption" {
  description             = "s3 kubeconfig encryption"
  tags = merge(
    local.tags_eks,
    {
      "Name" = "Kubeconfig Encryption"
    }
  )
}

resource "aws_s3_bucket_server_side_encryption_configuration" "kubeconfig_bucket_encryption" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id

  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.s3_kubeconfig_kms_encryption.arn
      sse_algorithm = "aws:kms"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "kubeconfigs_access" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id

  ignore_public_acls = true
  restrict_public_buckets  = true
  block_public_policy = true
  block_public_acls = true
}

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

resource "aws_s3_bucket_versioning" "vpc_flow_logs_versionning" {
  bucket = aws_s3_bucket.vpc_flow_logs.id
  versioning_configuration {
    status = "Disabled"
  }
}

resource "aws_s3_bucket_acl" "vpc_flow_logs_acl" {
  bucket = aws_s3_bucket.vpc_flow_logs.id
  acl    = "private"
}

resource "aws_kms_key" "s3_flow_logs_kms_encryption" {
  description             = "flow logs VPC encryption"
  tags = merge(
    local.tags_eks,
    {
      "Name" = "VPC flow logs Encryption"
    }
  )
}

resource "aws_s3_bucket_server_side_encryption_configuration" "flow_logs_bucket_encryption" {
  bucket = aws_s3_bucket.vpc_flow_logs.id

  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.s3_flow_logs_kms_encryption.arn
      sse_algorithm = "aws:kms"
    }
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