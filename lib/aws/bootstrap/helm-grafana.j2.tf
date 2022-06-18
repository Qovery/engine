resource "aws_iam_user" "iam_grafana_cloudwatch" {
  name = "qovery-cloudwatch-${var.kubernetes_cluster_id}"
  tags = local.tags_eks
}

resource "aws_iam_access_key" "iam_grafana_cloudwatch" {
  user    = aws_iam_user.iam_grafana_cloudwatch.name
}

resource "aws_iam_policy" "grafana_cloudwatch_policy" {
  name = aws_iam_user.iam_grafana_cloudwatch.name
  description = "Policy for K8s API/Scheduler logs visualisation from Cloudwatch"

  policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "AllowReadingMetricsFromCloudWatch",
      "Effect": "Allow",
      "Action": [
        "cloudwatch:DescribeAlarmsForMetric",
        "cloudwatch:DescribeAlarmHistory",
        "cloudwatch:DescribeAlarms",
        "cloudwatch:ListMetrics",
        "cloudwatch:GetMetricStatistics",
        "cloudwatch:GetMetricData"
      ],
      "Resource": "*"
    },
    {
      "Sid": "AllowReadingLogsFromCloudWatch",
      "Effect": "Allow",
      "Action": [
        "logs:DescribeLogGroups",
        "logs:GetLogGroupFields",
        "logs:StartQuery",
        "logs:StopQuery",
        "logs:GetQueryResults",
        "logs:GetLogEvents"
      ],
      "Resource": "*"
    },
    {
      "Sid": "AllowReadingTagsInstancesRegionsFromEC2",
      "Effect": "Allow",
      "Action": ["ec2:DescribeTags", "ec2:DescribeInstances", "ec2:DescribeRegions"],
      "Resource": "*"
    },
    {
      "Sid": "AllowReadingResourcesForTags",
      "Effect": "Allow",
      "Action": "tag:GetResources",
      "Resource": "*"
    }
  ]
}
POLICY
}

resource "aws_iam_user_policy_attachment" "grafana_cloudwatch_attachment" {
  user       = aws_iam_user.iam_grafana_cloudwatch.name
  policy_arn = aws_iam_policy.grafana_cloudwatch_policy.arn
}