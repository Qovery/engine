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

locals {
  cloudflare_datasources = <<DATASOURCES
datasources:
  datasources.yaml:
    apiVersion: 1
    datasources:
      - name: Prometheus
        type: prometheus
        url: "http://prometheus-operator-prometheus:9090"
        access: proxy
        isDefault: true
      - name: PromLoki
        type: prometheus
        url: "http://${helm_release.loki.name}.${helm_release.loki.namespace}.svc:3100/loki"
        access: proxy
        isDefault: false
      - name: Loki
        type: loki
        url: "http://${helm_release.loki.name}.${helm_release.loki.namespace}.svc:3100"
      - name: Cloudwatch
        type: cloudwatch
        jsonData:
          authType: keys
          defaultRegion: ${var.region}
        secureJsonData:
          accessKey: '${aws_iam_access_key.iam_grafana_cloudwatch.id}'
          secretKey: '${aws_iam_access_key.iam_grafana_cloudwatch.secret}'
DATASOURCES
}

resource "helm_release" "grafana" {
  name = "grafana"
  chart = "common/charts/grafana"
  namespace = "prometheus"
  atomic = true
  max_history = 50

  values = [
    file("chart_values/grafana.yaml"),
    local.cloudflare_datasources,
  ]

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.cluster_autoscaler,
    helm_release.aws_vpc_cni,
  ]
}
