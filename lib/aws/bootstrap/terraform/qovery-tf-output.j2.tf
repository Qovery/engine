output "aws_iam_eks_user_mapper_role_arn" { value = aws_iam_role.iam_eks_user_mapper.arn }
output "aws_iam_cluster_autoscaler_role_arn" { value = aws_iam_role.iam_eks_cluster_autoscaler.arn }
output "aws_iam_cloudwatch_role_arn" { value = aws_iam_role.iam_grafana_cloudwatch.arn }
output "loki_storage_config_aws_s3" { value = "s3://${var.region}/${aws_s3_bucket.loki_bucket.bucket}" }
output "aws_iam_loki_role_arn" { value = aws_iam_role.iam_eks_loki.arn }
output "aws_s3_loki_bucket_name" { value = aws_iam_role.iam_eks_loki.name }
output "aws_account_id" { value = data.aws_caller_identity.current.account_id }
output "karpenter_controller_aws_role_arn" { value = aws_iam_role.karpenter_controller_role.arn }
output "cluster_security_group_id" { value = aws_eks_cluster.eks_cluster.vpc_config[0].cluster_security_group_id }
output "aws_iam_alb_controller_arn" { value = aws_iam_role.aws_load_balancer_controller.arn }
output "aws_iam_eks_prometheus_role_arn" { value = aws_iam_role.iam_eks_prometheus.arn }
output "aws_s3_prometheus_bucket_name" { value = aws_s3_bucket.prometheus_bucket.id }
output "kubeconfig" {
  sensitive = true
  depends_on = [aws_eks_cluster.eks_cluster]
  value = <<KUBECONFIG
apiVersion: v1
clusters:
- cluster:
    server: ${aws_eks_cluster.eks_cluster.endpoint}
    certificate-authority-data: ${aws_eks_cluster.eks_cluster.certificate_authority.0.data}
  name: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
contexts:
- context:
    cluster: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
    user: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
  name: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
current-context: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
kind: Config
preferences: {}
users:
- name: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
  user:
    exec:
      apiVersion: client.authentication.k8s.io/v1
      interactiveMode: IfAvailable
      command: aws
      args:
        - "eks"
        - "get-token"
        - "--cluster-name"
        - "${aws_eks_cluster.eks_cluster.name}"
KUBECONFIG
}
