resource "aws_s3_bucket_object" "upload_kubeconfig" {
  bucket = var.s3_bucket_kubeconfig
  key = "${var.eks_cluster_id}.yaml"
  source = local_file.kubeconfig.filename
  server_side_encryption = "AES256"
  depends_on = [local_file.kubeconfig, aws_s3_bucket.kubeconfigs_bucket]

  tags = local.tags_eks
}
