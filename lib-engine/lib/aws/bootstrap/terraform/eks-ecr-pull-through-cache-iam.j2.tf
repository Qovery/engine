{% if aws_ecr_enable_pull_through_cache %}

# Pull-through cache misses create a private repository and import the upstream
# image under this Qovery-owned namespace. The managed read-only ECR policy
# already grants the remaining permissions needed to pull cached images.
resource "aws_iam_role_policy" "eks_workers_ecr_pull_through_cache" {
  name = "QoveryECRPullThroughCache-${var.kubernetes_cluster_id}"
  role = aws_iam_role.eks_workers.name
  policy = jsonencode(
    {
      "Version" : "2012-10-17",
      "Statement" : [
        {
          "Sid" : "AllowQoveryECRPullThroughCacheImport",
          "Effect" : "Allow",
          "Action" : [
            "ecr:BatchImportUpstreamImage",
            "ecr:CreateRepository"
          ],
          "Resource" : "arn:aws:ecr:${var.region}:${data.aws_caller_identity.current.account_id}:repository/qovery-ecr-public/*"
        }
      ]
    }
  )
}

resource "aws_iam_role_policy" "karpenter_nodes_ecr_pull_through_cache" {
  name = "QoveryECRPullThroughCache-${var.kubernetes_cluster_id}"
  role = aws_iam_role.karpenter_node_role.name
  policy = jsonencode(
    {
      "Version" : "2012-10-17",
      "Statement" : [
        {
          "Sid" : "AllowQoveryECRPullThroughCacheImport",
          "Effect" : "Allow",
          "Action" : [
            "ecr:BatchImportUpstreamImage",
            "ecr:CreateRepository"
          ],
          "Resource" : "arn:aws:ecr:${var.region}:${data.aws_caller_identity.current.account_id}:repository/qovery-ecr-public/*"
        }
      ]
    }
  )
}

{% endif %}
