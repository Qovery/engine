resource "aws_security_group" "ec2_cluster" {
  name        = "qovery-ec2-${var.kubernetes_cluster_id}"
  description = "Cluster communication with worker nodes"
  vpc_id      = aws_vpc.ec2.id

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = local.tags_ec2
}

resource "aws_security_group_rule" "https" {
  cidr_blocks       = "0.0.0.0/0"
  description       = "HTTPS connectivity"
  from_port         = 443
  protocol          = "tcp"
  security_group_id = aws_security_group.ec2_cluster.id
  to_port           = 443
  type              = "ingress"
}

resource "aws_security_group_rule" "ssh" {
  cidr_blocks       = "0.0.0.0/0"
  description       = "SSH remote access"
  from_port         = 22
  protocol          = "tcp"
  security_group_id = aws_security_group.ec2_cluster.id
  to_port           = 22
  type              = "ssh"
}