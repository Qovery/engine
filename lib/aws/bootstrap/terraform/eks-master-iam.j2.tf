#########################################################################################
# If the current session is assumed-role, we need to create an access entry for the role
# and associate the AmazonEKSClusterAdminPolicy to the role. Else we can't connect to the cluster
# If we are using an user, we need to attach the AmazonEKSClusterAdminPolicy to it
#
# When the wide permissions credentials are used, the current session is the wide permissions
# one: the cloud provider options identity keeps its access entry, and the wide permissions
# identity gets its own one
#########################################################################################

{% if aws_wide_permissions_enabled -%}
data "aws_caller_identity" "wide_permissions_options_credentials" {
  provider = aws.wide_permissions_options_credentials
}
{%- endif %}

locals {
{%- if aws_wide_permissions_enabled %}
  options_caller_arn = data.aws_caller_identity.wide_permissions_options_credentials.arn
{%- else %}
  options_caller_arn = data.aws_caller_identity.current.arn
{%- endif %}
  is_role = can(regex("assumed-role", local.options_caller_arn))
  account_id = data.aws_caller_identity.current.account_id
  role_name = local.is_role ? split("/", local.options_caller_arn)[length(split("/", local.options_caller_arn)) - 2] : ""

  principal_arn = local.is_role ? "arn:aws:iam::${local.account_id}:role/${local.role_name}" : local.options_caller_arn
  admin_policy_arn = "arn:aws:eks::aws:cluster-access-policy/AmazonEKSClusterAdminPolicy"
{%- if aws_wide_permissions_enabled %}

  wide_permissions_caller_arn = data.aws_caller_identity.current.arn
  wide_permissions_is_role = can(regex("assumed-role", local.wide_permissions_caller_arn))
  wide_permissions_role_name = local.wide_permissions_is_role ? split("/", local.wide_permissions_caller_arn)[length(split("/", local.wide_permissions_caller_arn)) - 2] : ""
  wide_permissions_principal_arn = local.wide_permissions_is_role ? "arn:aws:iam::${local.account_id}:role/${local.wide_permissions_role_name}" : local.wide_permissions_caller_arn
  # A principal can only have one access entry
  create_wide_permissions_access = local.wide_permissions_principal_arn != local.principal_arn
{%- endif %}
}

resource "aws_eks_access_entry" "qovery_eks_access" {
  cluster_name      = aws_eks_cluster.eks_cluster.name
  principal_arn     = local.principal_arn
  type              = "STANDARD"
  tags              = local.tags_eks

  depends_on = [aws_eks_cluster.eks_cluster]
}

resource "aws_eks_access_policy_association" "qovery_eks_access" {
  cluster_name      = aws_eks_cluster.eks_cluster.name
  principal_arn     = local.principal_arn
  policy_arn        = local.admin_policy_arn

  access_scope {
    type       = "cluster"
  }

  depends_on = [aws_eks_cluster.eks_cluster]
}

{% if aws_wide_permissions_enabled -%}
# Removed on the next deployment done without the wide permissions credentials
resource "aws_eks_access_entry" "qovery_eks_access_wide_permissions" {
  count             = local.create_wide_permissions_access ? 1 : 0
  cluster_name      = aws_eks_cluster.eks_cluster.name
  principal_arn     = local.wide_permissions_principal_arn
  type              = "STANDARD"
  tags              = local.tags_eks

  lifecycle {
    precondition {
      condition     = data.aws_caller_identity.wide_permissions_options_credentials.account_id == data.aws_caller_identity.current.account_id
      error_message = "The wide permissions credentials must belong to the same AWS account as the cloud provider credentials."
    }
  }

  depends_on = [aws_eks_cluster.eks_cluster]
}

resource "aws_eks_access_policy_association" "qovery_eks_access_wide_permissions" {
  count             = local.create_wide_permissions_access ? 1 : 0
  cluster_name      = aws_eks_cluster.eks_cluster.name
  principal_arn     = local.wide_permissions_principal_arn
  policy_arn        = local.admin_policy_arn

  access_scope {
    type       = "cluster"
  }

  depends_on = [aws_eks_access_entry.qovery_eks_access_wide_permissions]
}
{%- endif %}


#######
# IAM #
#######

resource "aws_iam_role" "eks_cluster" {
  name = "qovery-eks-${var.kubernetes_cluster_id}"

  tags = local.tags_eks

  assume_role_policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Service": "eks.amazonaws.com"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}
POLICY
}

resource "aws_iam_role_policy_attachment" "eks_cluster_AmazonEKSClusterPolicy" {
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSClusterPolicy"
  role       = aws_iam_role.eks_cluster.name
}

resource "aws_iam_role_policy_attachment" "eks_cluster_AmazonEKSServicePolicy" {
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSServicePolicy"
  role       = aws_iam_role.eks_cluster.name
}

{%- if aws_iam_user_mapper_sso_enabled -%}

# SSO
# Resources below allows SSO connection to kube cluster

resource "aws_iam_role" "iam_eks_cluster_creator_role_trust_role" {
  name = "qovery-eks-cluster-creator-role-trust-${var.kubernetes_cluster_id}"

  tags = local.tags_eks

  assume_role_policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "${var.aws_iam_user_mapper_sso_role_arn}"
      },
      "Action": "sts:AssumeRole",
      "Condition": {}
    }
  ]
}
POLICY
}

resource "aws_iam_policy" "iam_eks_cluster_creator_role_permissions_policy" {
  name = "qovery-eks-cluster-creator-role-permissions-policy-${var.kubernetes_cluster_id}"
  description = "Policy for cluster creator role permissions"

  policy = <<POLICY
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "eks:*",
                "iam:*",
                "cloudformation:*",
                "ec2:*",
                "autoscaling:*",
                "ssm:*",
                "kms:*",
                "sts:GetCallerIdentity"
            ],
            "Resource": "*"
        }
    ]
}
POLICY
}

resource "aws_iam_role_policy_attachment" "iam_eks_cluster_creator_role_permissions_policy" {
  policy_arn = aws_iam_policy.iam_eks_cluster_creator_role_permissions_policy.arn
  role       = aws_iam_role.iam_eks_cluster_creator_role_trust_role.name
}

{%- endif -%}