data "aws_caller_identity" "current" {}

locals {
  qovery_tf_config = <<TF_CONFIG
{
  "aws_ec2_public_hostname": "${aws_instance.ec2_instance.public_dns}",
  "aws_ec2_kubernetes_port": "${random_integer.kubernetes_external_port.result}",
  "aws_aws_account_id": "${data.aws_caller_identity.current.account_id}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}
