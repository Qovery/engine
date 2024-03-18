{% if enable_karpenter %}
locals {
  events = {
    health_event = {
      name        = "HealthEvent"
      description = "Karpenter interrupt - AWS health event"
      event_pattern = {
        source      = ["aws.health"]
        detail-type = ["AWS Health Event"]
      }
    }
    spot_interrupt = {
      name        = "SpotInterrupt"
      description = "Karpenter interrupt - EC2 spot instance interruption warning"
      event_pattern = {
        source      = ["aws.ec2"]
        detail-type = ["EC2 Spot Instance Interruption Warning"]
      }
    }
    instance_rebalance = {
      name        = "InstanceRebalance"
      description = "Karpenter interrupt - EC2 instance rebalance recommendation"
      event_pattern = {
        source      = ["aws.ec2"]
        detail-type = ["EC2 Instance Rebalance Recommendation"]
      }
    }
    instance_state_change = {
      name        = "InstanceStateChange"
      description = "Karpenter interrupt - EC2 instance state-change notification"
      event_pattern = {
        source      = ["aws.ec2"]
        detail-type = ["EC2 Instance State-change Notification"]
      }
    }
  }
}


resource "aws_sqs_queue" "qovery-eks-queue" {
  name                      = var.kubernetes_cluster_name
  message_retention_seconds = 300
  sqs_managed_sse_enabled   = true

  tags = merge(
    local.tags_common,
  )
}

data "aws_iam_policy_document" "queue" {
  statement {
    sid       = "SqsWrite"
    actions   = ["sqs:SendMessage"]
    resources = [aws_sqs_queue.qovery-eks-queue.arn]

    principals {
      type = "Service"
      identifiers = [
        "events.amazonaws.com",
        "sqs.amazonaws.com",
      ]
    }
  }
}

resource "aws_sqs_queue_policy" "qovery_sqs_queue_policy" {
  queue_url = aws_sqs_queue.qovery-eks-queue.url
  policy    = data.aws_iam_policy_document.queue.json
}

resource "aws_cloudwatch_event_rule" "qovery_cloudwatch_event_rule" {
  for_each = { for k, v in local.events : k => v }

  name_prefix   = "qovery-cw-event-${each.value.name}-"
  description   = each.value.description
  event_pattern = jsonencode(each.value.event_pattern)

  tags = merge(
    local.tags_common,
  )
}

resource "aws_cloudwatch_event_target" "qovery_cloudwatch_event_target" {
  for_each = { for k, v in local.events : k => v }

  rule      = aws_cloudwatch_event_rule.qovery_cloudwatch_event_rule[each.key].name
  target_id = "KarpenterInterruptionQueueTarget"
  arn       = aws_sqs_queue.qovery-eks-queue.arn
}
{% endif %}
