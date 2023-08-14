locals {
  qovery_tf_config = <<TF_CONFIG
{
  "aws_iam_eks_user_mapper_role_arn": "${aws_iam_role.iam_eks_user_mapper.arn}",
  "aws_iam_cluster_autoscaler_role_arn": "${aws_iam_role.iam_eks_cluster_autoscaler.arn}",
  "aws_iam_cloudwatch_role_arn": "${aws_iam_role.iam_grafana_cloudwatch.arn}",
  "loki_storage_config_aws_s3": "s3://${var.region}/${aws_s3_bucket.loki_bucket.bucket}",
  "aws_iam_loki_role_arn": "${aws_iam_role.iam_eks_loki.arn}",
  "aws_s3_loki_bucket_name": "${aws_iam_role.iam_eks_loki.name}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}
