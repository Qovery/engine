resource "aws_key_pair" "qovery_ssh_key_{{ kubernetes_cluster_id }}" {
  key_name = var.ec2_ssh_default_key.key_name
  public_key = var.ec2_ssh_default_key.public_key

  # Don't add tags, there is an issue, the key will disappear, making the autoscaler stuck because of this (in the best case)
  #tags = local.tags_eks
}
