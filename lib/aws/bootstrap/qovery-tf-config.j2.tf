locals {
  qovery_tf_config = <<TF_CONFIG
{
  "aws_iam_eks_user_mapper_role_arn": "${aws_iam_role.iam_eks_user_mapper.arn}",
  "aws_iam_cluster_autoscaler_role_arn": "${aws_iam_role.iam_eks_cluster_autoscaler.arn}",
  "aws_iam_cloudwatch_role_arn": "${aws_iam_role.iam_grafana_cloudwatch.arn}",
  "loki_storage_config_aws_s3": "s3://${var.region}/${aws_s3_bucket.loki_bucket.bucket}",
  "aws_iam_loki_role_arn": "${aws_iam_role.iam_eks_loki.arn}",
  "aws_s3_loki_bucket_name": "${aws_iam_role.iam_eks_loki.name}",
  "aws_account_id": "${data.aws_caller_identity.current.account_id}",
  "karpenter_controller_aws_role_arn": "${aws_iam_role.karpenter_controller_role.arn}",
  "cluster_security_group_id": "${aws_eks_cluster.eks_cluster.vpc_config[0].cluster_security_group_id}",
  "aws_iam_alb_controller_arn": "${aws_iam_role.aws_load_balancer_controller.arn}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}
