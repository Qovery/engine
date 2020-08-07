resource "aws_iam_role" "engine_resources_iam" {
  name = "s3-qengine-resources-${var.eks_cluster_id}"

  tags = local.tags_eks

  assume_role_policy = <<POLICY
{
   "Version":"2012-10-17",
   "Statement":[
      {
         "Effect":"Allow",
         "Action":[
            "s3:GetObject"
         ],
         "Resource":"arn:aws:s3:::${var.s3_bucket_qengine_resources}/*"
      }
   ]
}
POLICY
}