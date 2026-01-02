{% if enable_keda -%}

# KEDA Operator IAM Role
# Assumed by kube-system/keda-operator via IRSA
resource "aws_iam_role" "keda_operator" {
  name        = "qovery-keda-operator-irsa-${var.kubernetes_cluster_id}"
  description = "IAM Role for KEDA operator (IRSA). No AWS permissions; use per-scaler roleArn (TriggerAuthentication) for AWS access."
  tags        = local.tags_eks

  assume_role_policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "AllowKedaOperatorSA",
        "Effect" : "Allow",
        "Principal" : {
          "Federated" : "${aws_iam_openid_connect_provider.oidc.arn}"
        },
        "Action" : "sts:AssumeRoleWithWebIdentity",
        "Condition" : {
          "StringEquals" : {
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub" : "system:serviceaccount:kube-system:keda-operator",
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:aud" : "sts.amazonaws.com"
          }
        }
      }
    ]
  })
}

# No policy attachment on purpose:
# KEDA operator must not have SQS/CloudWatch/etc.
# AWS permissions are provided by per-scaler roles (e.g. keda-sqs-app1) referenced via TriggerAuthentication.roleArn.


# KEDA Metrics Server IAM Role
# Assumed by kube-system/keda-metrics-server via IRSA
resource "aws_iam_role" "keda_metrics_server" {
  name        = "qovery-keda-metrics-irsa-${var.kubernetes_cluster_id}"
  description = "IAM Role for KEDA metrics server (IRSA). No AWS permissions."
  tags        = local.tags_eks

  assume_role_policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "AllowKedaMetricsServerSA",
        "Effect" : "Allow",
        "Principal" : {
          "Federated" : "${aws_iam_openid_connect_provider.oidc.arn}"
        },
        "Action" : "sts:AssumeRoleWithWebIdentity",
        "Condition" : {
          "StringEquals" : {
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub" : "system:serviceaccount:kube-system:keda-metrics-server",
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:aud" : "sts.amazonaws.com"
          }
        }
      }
    ]
  })
}

{% endif -%}
