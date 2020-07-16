# Because it needs to be uniq across all clusters and Terraform doesn't brings solution to this, I'm using this hack
data "external" "create-es-role" {
  program = ["./helper.sh", "create_es_role_for_aws_service", "AWSServiceRoleForAmazonElasticsearchService", "es.amazonaws.com"]
}

# Network

resource "aws_subnet" "es-zone-a" {
  count = var.es_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[0]
  cidr_block = "10.0.${count.index * 2 + 184}.0/${var.es_cidr_subnet}"
  vpc_id = aws_vpc.eks.id

  tags = aws_security_group.elasticsearch.tags
}

resource "aws_subnet" "es-zone-b" {
  count = var.es_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[1]
  cidr_block = "10.0.${count.index * 2 + 188}.0/${var.es_cidr_subnet}"
  vpc_id = aws_vpc.eks.id

  tags = aws_security_group.elasticsearch.tags
}

resource "aws_subnet" "es-zone-c" {
  count = var.es_nb_subnets_per_zone - 1

  availability_zone = data.aws_availability_zones.available.names[2]
  cidr_block = "10.0.${count.index * 2 + 192}.0/${var.es_cidr_subnet}"
  vpc_id = aws_vpc.eks.id

  tags = aws_security_group.elasticsearch.tags
}

resource "aws_route_table_association" "es_cluster-zone-a" {
  count = var.es_nb_subnets_per_zone

  subnet_id      = aws_subnet.es-zone-a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "es_cluster-zone-b" {
  count = var.es_nb_subnets_per_zone

  subnet_id      = aws_subnet.es-zone-b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "es_cluster-zone-c" {
  count = var.es_nb_subnets_per_zone - 1

  subnet_id      = aws_subnet.es-zone-c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_security_group" "elasticsearch" {
  name = "${var.eks_cluster_id}-elasticsearch"
  description = "Elasticsearch security group"
  vpc_id = aws_vpc.eks.id

  ingress {
    from_port = 443
    to_port = 443
    protocol = "tcp"

    cidr_blocks = [
      aws_vpc.eks.cidr_block
    ]
  }

  tags = merge(
    aws_eks_cluster.eks_cluster.tags,
    {
      "Service" = "Elasticsearch"
    }
  )
}