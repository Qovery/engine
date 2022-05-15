// S3 bucket to store kubeconfigs
resource "aws_s3_bucket" "kubeconfigs_bucket" {
  bucket = var.s3_bucket_kubeconfig
  force_destroy = true

  tags = merge(
    local.tags_ec2,
    {
      "Name" = "Kubernetes kubeconfig"
    }
  )
}

resource "aws_s3_bucket_acl" "kubeconfigs_bucket_acl" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id
  acl    = "private"
}

resource "aws_s3_bucket_server_side_encryption_configuration" "kubeconfigs_bucket_encryption" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id
  rule {
    apply_server_side_encryption_by_default {
      kms_master_key_id = aws_kms_key.s3_kubeconfig_kms_encryption.arn
      sse_algorithm     = "aws:kms"
    }
  }
}

resource "aws_s3_bucket_versioning" "kubeconfigs_bucket_versionning" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_kms_key" "s3_kubeconfig_kms_encryption" {
  description             = "s3 kubeconfig encryption"
  tags = merge(
    local.tags_ec2,
    {
      "Name" = "Kubeconfig Encryption"
    }
  )
}

resource "aws_s3_bucket_public_access_block" "kubeconfigs_access" {
  bucket = aws_s3_bucket.kubeconfigs_bucket.id

  ignore_public_acls = true
  restrict_public_buckets  = true
  block_public_policy = true
  block_public_acls = true
}