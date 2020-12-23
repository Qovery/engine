resource "aws_key_pair" "qovery_ssh_key_{{ kubernetes_cluster_id }}" {
  key_name = var.ec2_ssh_default_key.key_name
  public_key = var.ec2_ssh_default_key.public_key
}
