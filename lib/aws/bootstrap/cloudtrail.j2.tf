resource "aws_cloudtrail" "trail" {
  name                    = "qovery-trail"
  s3_bucket_name          = var.s3_trail_bucket_name
  is_multi_region_trail   = var.is_multi_region_trail
  tags                    = local.tags_eks
  depends_on              = [aws_s3_bucket.trail]
}
#
# S3 bucket for cloudtrails log
#
resource "aws_s3_bucket" "trail" {
  bucket = var.s3_trail_bucket_name

  lifecycle_rule {
    enabled = true

    expiration {
      days = var.s3_bucket_trail_days_to_expiration
    }
  }
}

resource "aws_s3_bucket_policy" "trail" {
  bucket = aws_s3_bucket.trail.id
  policy = data.aws_iam_policy_document.cloudtrail_log_access.json
}

#
# Access policy for CloudTrail <> S3
#
data "aws_iam_policy_document" "cloudtrail_log_access" {
  statement {
    sid       = "AWSCloudTrailAclCheck"
    actions   = ["s3:GetBucketAcl"]
    resources = [aws_s3_bucket.trail.arn]

    principals {
      type        = "Service"
      identifiers = ["cloudtrail.amazonaws.com"]
    }
  }

  statement {
    sid     = "AWSCloudTrailWrite"
    actions = ["s3:PutObject"]

    resources = [format("%s/*", aws_s3_bucket.trail.arn)]

    principals {
      type        = "Service"
      identifiers = ["cloudtrail.amazonaws.com"]
    }

    condition {
      test     = "StringEquals"
      variable = "s3:x-amz-acl"
      values   = ["bucket-owner-full-control"]
    }
  }
}