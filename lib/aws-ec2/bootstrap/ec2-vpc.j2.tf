# Public subnets
resource "aws_subnet" "ec2_zone_a" {
  count = length(var.ec2_subnets_zone_a_private)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.ec2_subnets_zone_a_private[count.index]
  vpc_id = aws_vpc.ec2.id
  map_public_ip_on_launch = true

  tags = local.tags_ec2_vpc
}

resource "aws_subnet" "ec2_zone_b" {
  count = length(var.ec2_subnets_zone_b_private)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.ec2_subnets_zone_b_private[count.index]
  vpc_id = aws_vpc.ec2.id
  map_public_ip_on_launch = true

  tags = local.tags_ec2_vpc
}

resource "aws_subnet" "ec2_zone_c" {
  count = length(var.ec2_subnets_zone_c_private)

  availability_zone = var.aws_availability_zones[2]
  cidr_block = var.ec2_subnets_zone_c_private[count.index]
  vpc_id = aws_vpc.ec2.id
  map_public_ip_on_launch = true

  tags = local.tags_ec2_vpc
}

resource "aws_route_table" "ec2_cluster" {
  vpc_id = aws_vpc.ec2.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.ec2_instance.id
  }

  {% for route in vpc_custom_routing_table %}
  route {
    cidr_block = "{{ route.destination }}"
    gateway_id = "{{ route.target }}"
  }
  {% endfor %}

  tags = local.tags_ec2_vpc
}

resource "aws_route_table_association" "ec2_cluster_zone_a" {
  count = length(var.ec2_subnets_zone_a_private)

  subnet_id = aws_subnet.ec2_zone_a.*.id[count.index]
  route_table_id = aws_route_table.ec2_cluster.id
}

resource "aws_route_table_association" "ec2_cluster_zone_b" {
  count = length(var.ec2_subnets_zone_b_private)

  subnet_id = aws_subnet.ec2_zone_b.*.id[count.index]
  route_table_id = aws_route_table.ec2_cluster.id
}

resource "aws_route_table_association" "ec2_cluster_zone_c" {
  count = length(var.ec2_subnets_zone_c_private)

  subnet_id = aws_subnet.ec2_zone_c.*.id[count.index]
  route_table_id = aws_route_table.ec2_cluster.id
}