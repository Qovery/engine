resource "aws_iam_user" "qovery_engine_resources" {
  name = "qovery-engine-resources-${var.eks_cluster_id}"

  tags = local.tags_eks
}

resource "aws_iam_access_key" "qovery_engine_resources" {
  user    = aws_iam_user.qovery_engine_resources.name
}

resource "aws_iam_user_policy" "qovery_engine_resources" {
  name = aws_iam_user.qovery_engine_resources.name
  user = aws_iam_user.qovery_engine_resources.name

  policy = <<EOF
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
EOF
}

resource "helm_release" "qovery_engine_resources" {
  name = "qovery-engine"
  chart = "common/charts/qovery-engine"
  namespace = "qovery"
  atomic = true
  create_namespace = true
  max_history = 50

  set {
    name = "image.tag"
    value = "af42789"
  }

  set {
    name = "environmentVariables.resources-accessKey"
    value = aws_iam_access_key.qovery_engine_resources.id
  }

  set {
    name = "environmentVariables.resources-secretKey"
    value = aws_iam_access_key.qovery_engine_resources.secret
  }

  set {
    name = "environmentVariables.resources-url"
    value = "s3://"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    aws_iam_access_key.qovery_engine_resources
  ]
}