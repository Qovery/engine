#########################################################################################
# If the current session is assumed-role, we need to create an access entry for the role
# and associate the AmazonEKSClusterAdminPolicy to the role. Else we can't connect to the cluster
# If we are using an user, we need to attach the AmazonEKSClusterAdminPolicy to it
#########################################################################################

locals {
  is_role = can(regex("assumed-role", data.aws_caller_identity.current.arn))
  account_id = data.aws_caller_identity.current.account_id
  role_name = local.is_role ? split("/", data.aws_caller_identity.current.arn)[length(split("/", data.aws_caller_identity.current.arn)) - 2] : ""

  principal_arn = local.is_role ? "arn:aws:iam::${local.account_id}:role/${local.role_name}" : data.aws_caller_identity.current.arn
  admin_policy_arn = "arn:aws:eks::aws:cluster-access-policy/AmazonEKSClusterAdminPolicy"
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