resource "aws_elasticsearch_domain" "qovery_eks_logs" {
  domain_name           = var.es_q_logs_domain_name
  elasticsearch_version = "7.4"

  cluster_config {
    instance_type = "m5.large.elasticsearch"
    instance_count = var.es_nodes_number
  }

  vpc_options {
    subnet_ids = [aws_subnet.es-zone-a.*.id[0]]
    security_group_ids = [ aws_security_group.elasticsearch.id ]
  }

  ebs_options {
    ebs_enabled = true
    volume_size = var.es_volume_size
  }

  advanced_options = {
    "rest.action.multi.allow_explicit_index" = "true"
  }

  access_policies = <<CONFIG
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "*"
      },
      "Action": "es:*",
      "Resource": "arn:aws:es:${var.region}:283389881690:domain/qlogs-${var.eks_cluster_id}/*"
    },
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::283389881690:user/fluentbit-forward2es"
      },
      "Action": "es:*",
      "Resource": "arn:aws:es:${var.region}:283389881690:domain/qlogs-${var.eks_cluster_id}/*"
    }
  ]
}
CONFIG

  snapshot_options {
    automated_snapshot_start_hour = 3
  }

  tags = merge(
    local.tags_eks,
    {
      "EsDomain" = var.es_q_logs_domain_name
    }
  )

  depends_on = [
    data.external.create-es-role,
    aws_security_group.elasticsearch
  ]
}
