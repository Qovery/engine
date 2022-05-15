resource "aws_iam_user" "iam_eks_user_mapper" {
  name = "qovery-aws-iam-eks-user-mapper-${var.kubernetes_cluster_id}"

  tags = local.tags_eks
}

resource "aws_iam_access_key" "iam_eks_user_mapper" {
  user    = aws_iam_user.iam_eks_user_mapper.name
}

resource "aws_iam_user_policy" "iam_eks_user_mapper" {
  name = aws_iam_user.iam_eks_user_mapper.name
  user = aws_iam_user.iam_eks_user_mapper.name

  policy = <<EOF
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "iam:GenerateCredentialReport",
                "iam:GenerateServiceLastAccessedDetails",
                "iam:Get*",
                "iam:List*",
                "iam:SimulateCustomPolicy",
                "iam:SimulatePrincipalPolicy"
            ],
            "Resource": "*"
        }
    ]
}
EOF
}