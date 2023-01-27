resource "aws_eks_addon" "vpc_cni" {
  cluster_name         = aws_eks_cluster.eks_cluster.name
  addon_name           = "vpc-cni"
  addon_version        = "v1.12.0-eksbuild.1" # https://docs.aws.amazon.com/eks/latest/userguide/managing-vpc-cni.html
  resolve_conflicts    = "OVERWRITE"

  # Get configuration fields: `aws eks describe-addon-configuration --addon-name vpc-cni --addon-version v1.12.0-eksbuild.1`
  # jq .configurationSchema --raw-output | jq .definitions
  # Note: it seems to miss some ENV VARs presents / supported on the plugin: CF https://github.com/aws/amazon-vpc-cni-k8s
  configuration_values = jsonencode({
    env = {
      # Not listed via describe-addon-configuration
      # =========
      # MINIMUM_IP_TARGET = 60 # number of total IP addresses that the daemon should attempt to allocate for pod assignment on the node (init phase)
      # WARM_IP_TARGET: 10 # number of free IP addresses that the daemon should attempt to keep available for pod assignment on the node
      # MAX_ENI: 100 # maximum number of ENIs that will be attached to the node (k8s recommend to avoid going over 100)
      # =========
    }
    resources = {
      requests = {
         cpu = "50m"
       }
    }
  }) 

  tags = local.tags_eks
}
