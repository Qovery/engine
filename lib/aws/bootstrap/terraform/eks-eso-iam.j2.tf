{% if enable_automatic_external_secrets_access -%}

# External Secrets Operator Role IAM
# Assumed by kube-system/keda-operator via IRSA
resource "aws_iam_role" "external_secrets_operator_role" {
  name        = "qovery-external-secrets-operator-irsa-${var.kubernetes_cluster_id}"
  description = "IAM Role for External Secrets Operator (IRSA)"
  tags        = local.tags_eks

  assume_role_policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "AllowExternalSecretsOperatorSA",
        "Effect" : "Allow",
        "Principal" : {
          "Federated" : "${aws_iam_openid_connect_provider.oidc.arn}"
        },
        "Action" : "sts:AssumeRoleWithWebIdentity",
        "Condition" : {
          "StringEquals" : {
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub" : "system:serviceaccount:qovery:external-secrets-operator-sa",
            "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:aud" : "sts.amazonaws.com"
          }
        }
      }
    ]
  })
}

resource "aws_iam_policy" "external_secrets_operator_iam_policy" {
  name = aws_iam_role.external_secrets_operator_role.name
  description = "Policy for External Secrets Operator"

  policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "ExternalSecretsOperator",
        "Effect" : "Allow",
        "Action" : concat(
          {% if enable_secrets_manager_iam_permissions -%}
          [
            "secretsmanager:GetResourcePolicy",
            "secretsmanager:GetSecretValue",
            "secretsmanager:DescribeSecret",
            "secretsmanager:ListSecretVersionIds",
            "secretsmanager:ListSecrets"
          ],
          {% else -%}
          [],
          {% endif -%}
          {% if enable_parameter_store_iam_permissions -%}
          [
            "ssm:GetParameter*",
            "ssm:ListTagsForResource"
          ],
          {% else -%}
          [],
          {% endif -%}
        ),
        "Resource" : "*"
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "external_secrets_operator_policy_attachment" {
  role       = aws_iam_role.external_secrets_operator_role.name
  policy_arn = aws_iam_policy.external_secrets_operator_iam_policy.arn
}

{% endif -%}
