data "aws_caller_identity" "current" {}

output "aws_ec2_public_hostname" { value = aws_instance.ec2_instance.public_dns }
output "aws_aws_account_id" { value = data.aws_caller_identity.current.account_id }
output "aws_iam_alb_controller_arn" { value = data.aws_caller_identity.current.account_id }
output "aws_ec2_kubernetes_port" {
  {% if is_old_k3s_version %}
    value = "${random_integer.kubernetes_external_port.result}"
  {% else %}
    value = "${var.k3s_config.exposed_port}"
  {%- endif %}
}