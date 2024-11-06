/*
 Adding a policy to cluster IAM role that allow permissions
 required to create AWSServiceRoleForElasticLoadBalancing service-linked role by EKS during ELB provisioning
 https://github.com/terraform-aws-modules/terraform-aws-eks/issues/183
*/

resource "aws_iam_role_policy" "eks_cluster_ingress_loadbalancer_creation" {
  name   = "ingress-loadbalancer-creation-${var.kubernetes_cluster_id}"
  role       = aws_iam_role.eks_cluster.name

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
