resource "aws_s3_bucket_object" "upload-kubeconfig" {
  bucket = "${var.region}-qovery-kubeconfigs"
  key = "aws_${var.region_cluster_name}.yaml"
  source = "kubeconfig/aws_${var.region_cluster_name}.yaml"
  server_side_encryption = "AES256"
  depends_on = [local_file.kubeconfig]
}
