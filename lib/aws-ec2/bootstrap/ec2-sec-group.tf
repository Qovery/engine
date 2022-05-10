# randomize inbound kubernetes port number for more security
resource "random_integer" "kubernetes_external_port" {
  min = 1024
  max = 65534
}

resource "aws_security_group" "ec2_instance" {
  name        = "qovery-ec2-${var.kubernetes_cluster_id}"
  description = "Cluster communication with worker nodes"
  vpc_id      = aws_vpc.ec2.id

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  // nginx ingress
  ingress {
    description = "HTTPS connectivity"
    from_port   = 443
    protocol    = "tcp"
    to_port     = 443
    cidr_blocks = ["0.0.0.0/0"]
  }

  // kubernetes
  ingress {
    description = "Kubernetes connectivity"
    from_port   = random_integer.kubernetes_external_port.result
    protocol    = "tcp"
    to_port     = random_integer.kubernetes_external_port.result
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = local.tags_ec2
}