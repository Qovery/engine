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

resource "helm_release" "iam_eks_user_mapper" {
  name = "iam-eks-user-mapper"
  chart = "charts/iam-eks-user-mapper"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "aws.accessKey"
    value = aws_iam_access_key.iam_eks_user_mapper.id
  }

  set {
    name = "aws.secretKey"
    value = aws_iam_access_key.iam_eks_user_mapper.secret
  }

  set {
    name = "aws.region"
    value = var.region
  }

  set {
    name = "syncIamGroup"
    value = "Admins"
  }

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "20m"
  }

  set {
    name = "resources.requests.cpu"
    value = "10m"
  }

  set {
    name = "resources.limits.memory"
    value = "32Mi"
  }

  set {
    name = "resources.requests.memory"
    value = "32Mi"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
  ]
}
