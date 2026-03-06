{% if enable_karpenter %}

# Dedicated nodegroup for Karpenter controller
# This nodegroup replaces Fargate for running Karpenter

resource "aws_launch_template" "karpenter_nodegroup" {
  name_prefix = "karpenter-controller-${var.kubernetes_cluster_id}-"

  metadata_options {
    http_endpoint = "enabled"
    http_tokens = var.ec2_metadata_imds_version
    # https://github.com/kubernetes/autoscaler/issues/3592
    # hop limit should be set to 2 for https://kubernetes-sigs.github.io/aws-load-balancer-controller/v2.4/deploy/installation/#using-the-amazon-ec2-instance-metadata-server-version-2-imdsv2
    http_put_response_hop_limit = 2
    instance_metadata_tags = "enabled"
  }

  # Add security group configuration for proper EKS communication
  vpc_security_group_ids = [aws_eks_cluster.eks_cluster.vpc_config[0].cluster_security_group_id]

  # Bottlerocket user data is merged with Amazon EKS managed user data (not replaced).
  # The configuration provided here overrides any settings configured by Amazon EKS.
  # See: https://docs.aws.amazon.com/eks/latest/userguide/launch-templates.html
  user_data = base64encode(<<-TOML
    [settings.kubernetes]
    max-pods = 17
    TOML
  )


  block_device_mappings {
    device_name = "/dev/xvda"

    ebs {
      volume_size = 20
      encrypted   = true
      volume_type = "gp3"
      iops        = 3000
      throughput  = 125
    }
  }

  # Bottlerocket uses a second volume for container data (images, logs)
  block_device_mappings {
    device_name = "/dev/xvdb"

    ebs {
      volume_size = 20
      encrypted   = true
      volume_type = "gp3"
      iops        = 3000
      throughput  = 125
    }
  }

  tags = local.tags_eks
  tag_specifications {
    resource_type = "instance"
    tags = merge(
      local.tags_eks,
      {
        "Name" = "qovery-${var.kubernetes_cluster_id}-karpenter-controller"
        "node.qovery.com/infrastructure" = "true"
      }
    )
  }
}

resource "aws_eks_node_group" "karpenter_controller" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  version                = var.eks_k8s_versions.workers
  node_role_arn          = aws_iam_role.karpenter_node_role.arn
  node_group_name        = "karpenter-controller-${var.kubernetes_cluster_id}"

  # Use regular EKS subnets for Karpenter controller (EC2 instances, not Fargate)
  {% if user_provided_network -%}
  subnet_ids             = flatten([data.aws_subnet.eks_zone_a[*].id, data.aws_subnet.eks_zone_b[*].id, data.aws_subnet.eks_zone_c[*].id])
  {%- else -%}
  subnet_ids             = flatten([aws_subnet.eks_zone_a[*].id, aws_subnet.eks_zone_b[*].id, aws_subnet.eks_zone_c[*].id])
  {%- endif %}

  instance_types         = ["t4g.medium", "m6g.medium"]
  ami_type               = "BOTTLEROCKET_ARM_64"

  tags = merge(
    local.tags_eks,
    {
      "QoveryNodeGroupId" = "${var.kubernetes_cluster_id}-karpenter"
      "QoveryNodeGroupName" = "karpenter-controller"
      "node.qovery.com/infrastructure" = "true"
    }
  )

  launch_template {
    id      = aws_launch_template.karpenter_nodegroup.id
    version = aws_launch_template.karpenter_nodegroup.latest_version
  }

  scaling_config {
    desired_size = 2
    max_size     = 2
    min_size     = 2
  }

  # Apply taints to ensure only Karpenter runs on these nodes
  taint {
    key    = "node.qovery.com/infrastructure"
    value  = "true"
    effect = "NO_SCHEDULE"
  }

  labels = {
    "node.qovery.com/infrastructure" = "true"
  }

  update_config {
    max_unavailable_percentage = 50  # Allow 1 node to be unavailable during updates
  }

  timeouts {
    create = "10m"
    delete = "60m"
    update = "60m"
  }

  # Ensure that IAM Role permissions are created before and deleted after EKS Node Group handling
  depends_on = [
    aws_iam_role_policy_attachment.karpenter_eks_worker_policy_node,
    aws_iam_role_policy_attachment.karpenter_eks_cni_policy,
    aws_iam_role_policy_attachment.karpenter_ec2_container_registry_read_only,
    aws_eks_cluster.eks_cluster,
  ]
}

# We don't need to suspend AZ rebalancing for Karpenter nodegroup since it's critical infrastructure
# and we want to maintain HA

{% endif %}