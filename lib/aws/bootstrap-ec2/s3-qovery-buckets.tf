// S3 bucket to store kubeconfigs
resource "aws_s3_bucket" "kubeconfigs_bucket" {
  bucket = var.s3_bucket_kubeconfig
  acl    = "private"
  force_destroy = true
  versioning {
    enabled = true
  }

  tags = merge(
    local.tags_ec2,
    {
      "Name" = "Kubernetes kubeconfig"
    }
  )

  server_side_encryption_configuration {
    rule {
      apply_server_side_encryption_by_default {
        kms_master_key_id = aws_kms_key.s3_kubeconfig_kms_encryption.arn
        sse_algorithm = "aws:kms"
      }
    }
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