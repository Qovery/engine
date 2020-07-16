/*
 Adding a policy to cluster IAM role that allow permissions
 required to create AWSServiceRoleForElasticLoadBalancing service-linked role by EKS during ELB provisioning
 https://github.com/terraform-aws-modules/terraform-aws-eks/issues/183
*/

resource "aws_iam_role_policy" "eks_cluster_ingress_loadbalancer_creation" {
  name   = "eks-cluster-ingress-loadbalancer-creation"
  role       = aws_iam_role.eks_cluster.name

  tags = aws_eks_cluster.eks_cluster.tags

  policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "ec2:DescribeAccountAttributes",
        "ec2:DescribeInternetGateways"
      ],
      "Resource": "*"
    }
  ]
}
POLICY
}

resource "helm_release" "nginx-ingress" {
  name = "nginx-ingress"
  chart = "../../../lib/common/charts/nginx-ingress"
  namespace = "nginx-ingress"
  create_namespace = true
  atomic = true
  max_history = 50

  # Because of NLB, svc can take some time to start
  timeout = 300
  values = [file("chart_values/nginx-ingress.yaml")]

  depends_on = [
    aws_iam_role_policy.eks_cluster_ingress_loadbalancer_creation,
    aws_eks_cluster.eks_cluster,
    helm_release.prometheus-operator
  ]
}