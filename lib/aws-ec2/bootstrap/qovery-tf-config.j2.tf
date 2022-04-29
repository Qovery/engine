locals {
  qovery_tf_config = <<TF_CONFIG
{
  "aws_iam_eks_user_mapper_key": "",
  "aws_iam_eks_user_mapper_secret": "",
  "aws_iam_cluster_autoscaler_key": "",
  "aws_iam_cluster_autoscaler_secret": "",
  "aws_iam_cloudwatch_key": "",
  "aws_iam_cloudwatch_secret": "",
  "loki_storage_config_aws_s3": "",
  "aws_iam_loki_storage_key": "",
  "aws_iam_loki_storage_secret": ""
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}
