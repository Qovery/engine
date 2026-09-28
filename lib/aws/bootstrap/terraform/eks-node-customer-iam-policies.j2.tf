# Attach the customer's managed policies to both Qovery node roles. The
# policy ARN is the Terraform instance key, so reconciliation removes an
# attachment when its ARN is removed from the cluster advanced setting.
data "aws_partition" "current" {}

locals {
  customer_node_iam_policy_arns = toset({{ aws_eks_node_iam_policy_arns | json_encode }})
}

resource "aws_iam_role_policy_attachment" "eks_workers_customer_policy" {
  for_each   = local.customer_node_iam_policy_arns
  role       = aws_iam_role.eks_workers.name
  policy_arn = each.value

  lifecycle {
    precondition {
      condition     = startswith(each.value, "arn:${data.aws_partition.current.partition}:iam::${data.aws_caller_identity.current.account_id}:policy/")
      error_message = "Node IAM policies must be customer-managed policies in the cluster's AWS account and partition."
    }
  }
}

resource "aws_iam_role_policy_attachment" "karpenter_nodes_customer_policy" {
  for_each   = local.customer_node_iam_policy_arns
  role       = aws_iam_role.karpenter_node_role.name
  policy_arn = each.value

  lifecycle {
    precondition {
      condition     = startswith(each.value, "arn:${data.aws_partition.current.partition}:iam::${data.aws_caller_identity.current.account_id}:policy/")
      error_message = "Node IAM policies must be customer-managed policies in the cluster's AWS account and partition."
    }
  }
}
