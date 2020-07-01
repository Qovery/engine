//resource "aws_iam_user" "iam-aws-limits-exporter" {
//  name = "aws-limits-exporter-${var.region_cluster_name}"
//}
//
//resource "aws_iam_access_key" "iam-aws-limits-exporter" {
//  user    = aws_iam_user.iam-aws-limits-exporter.name
//}
//
//resource "helm_release" "aws-limits-exporter" {
//  name = "aws-limits-exporter"
//  chart = "../charts/aws-limits-exporter"
//  namespace = "prometheus"
//  atomic = true
//  max_history = 50
//
//  set {
//    name = "awsCredentials.awsAccessKey"
//    value = aws_iam_access_key.iam-aws-limits-exporter.id
//  }
//
//  set {
//    name = "awsCredentials.awsSecretKey"
//    value = aws_iam_access_key.iam-aws-limits-exporter.secret
//  }
//
//  depends_on = [aws_eks_node_group.eks-cluster-workers]
//}