locals {
  qovery_tf_config = <<TF_CONFIG
{
  "aws_iam_eks_user_mapper_key": "${aws_iam_access_key.iam_eks_user_mapper.id}",
  "aws_iam_eks_user_mapper_secret": "${aws_iam_access_key.iam_eks_user_mapper.secret}",
  "aws_iam_cluster_autoscaler_role_arn": "${aws_iam_role.iam_eks_cluster_autoscaler.arn}",
  "aws_iam_cloudwatch_role_arn": "${aws_iam_role.iam_grafana_cloudwatch.arn}",
  "loki_storage_config_aws_s3": "s3://${urlencode(aws_iam_access_key.iam_eks_loki.id)}:${urlencode(aws_iam_access_key.iam_eks_loki.secret)}@${var.region}/${aws_s3_bucket.loki_bucket.bucket}",
  "aws_iam_loki_storage_key": "${aws_iam_access_key.iam_eks_loki.id}",
  "aws_iam_loki_storage_secret": "${aws_iam_access_key.iam_eks_loki.secret}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}
