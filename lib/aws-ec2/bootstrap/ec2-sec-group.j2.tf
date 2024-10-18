# randomize inbound kubernetes port number for more security
resource "random_integer" "kubernetes_external_port" {
  min = 1024
  # not more to avoid k3s and Kubernetes port overlap
  max = 9999
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

  // cert-manager
  ingress {
    description = "HTTP challenge"
    from_port   = 80
    protocol    = "tcp"
    to_port     = 80
    cidr_blocks = ["0.0.0.0/0"]
  }

  // kubernetes
{% if is_old_k3s_version %}
  ingress {
    description = "Kubernetes access"
    from_port   = random_integer.kubernetes_external_port.result
    protocol    = "tcp"
    to_port     = random_integer.kubernetes_external_port.result
    cidr_blocks = ["0.0.0.0/0"]
  }
{% else %}
  ingress {
    description = "Kubernetes access"
    from_port   = var.k3s_config.exposed_port
    protocol    = "tcp"
    to_port     = var.k3s_config.exposed_port
    cidr_blocks = ["0.0.0.0/0"]
  }
{%- endif %}

  // SSH
  ingress {
    description = "SSH access"
    from_port   = 22
    protocol    = "tcp"
    to_port     = 22
    cidr_blocks = ["0.0.0.0/0"]
  }

  // MySQL
{% if not database_mysql_deny_any_access -%}
  ingress {
    description = "MySQL access"
    from_port   = 3306
    protocol    = "tcp"
    to_port     = 3306
    cidr_blocks = var.database_mysql_allowed_cidrs
  }
{% endif -%}

  // PostgreSQL
{% if not database_postgresql_deny_any_access -%}
  ingress {
    description = "PostgreSQL access"
    from_port   = 5432
    protocol    = "tcp"
    to_port     = 5432
    cidr_blocks = var.database_postgresql_allowed_cidrs
  }
{% endif -%}

  // MongoDB
{% if not database_mongodb_deny_any_access -%}
  ingress {
    description = "MongoDB access"
    from_port   = 27017
    protocol    = "tcp"
    to_port     = 27017
    cidr_blocks = var.database_mongodb_allowed_cidrs
  }
{% endif -%}

  // Redis
{% if not database_redis_deny_any_access -%}
  ingress {
    description = "Redis access"
    from_port   = 6379
    protocol    = "tcp"
    to_port     = 6379
    cidr_blocks = var.database_redis_allowed_cidrs
  }
{% endif -%}

  tags = local.tags_ec2
}

