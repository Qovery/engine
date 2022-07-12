{% if vpc_qovery_network_mode == "WithoutNatGateways" and user_provided_network %}

# Network is provided by the user
variable "eks_subnets_zone_a_ids" {
  type    = list(string)
  default = [
    {%- for id in eks_subnets_zone_a_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "eks_subnets_zone_b_ids" {
  type    = list(string)
  default = [
    {%- for id in eks_subnets_zone_b_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "eks_subnets_zone_c_ids" {
  type    = list(string)
  default = [
    {%- for id in eks_subnets_zone_c_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

# Public subnets
data "aws_subnet" "eks_zone_a" {
  count = length(var.eks_subnets_zone_a_ids)
  id    = var.eks_subnets_zone_a_ids[count.index]
}

data "aws_subnet" "eks_zone_b" {
  count = length(var.eks_subnets_zone_b_ids)
  id    = var.eks_subnets_zone_b_ids[count.index]
}

data "aws_subnet" "eks_zone_c" {
  count = length(var.eks_subnets_zone_c_ids)
  id    = var.eks_subnets_zone_c_ids[count.index]
}

resource "aws_route_table" "eks_cluster" {
  vpc_id = data.aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = data.aws_internet_gateway.eks_cluster.id
  }

// todo(pmavro): add tests for it when it will be available in the SDK
  {% for route in vpc_custom_routing_table %}
  route {
    cidr_block = "{{ route.destination }}"
    gateway_id = "{{ route.target }}"
  }
  {% endfor %}

  tags = local.tags_eks_vpc
}

resource "aws_route_table_association" "eks_cluster_zone_a" {
  count = length(var.eks_subnets_zone_a_ids)

  subnet_id = data.aws_subnet.eks_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_b" {
  count = length(var.eks_subnets_zone_b_ids)

  subnet_id = data.aws_subnet.eks_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_c" {
  count = length(var.eks_subnets_zone_c_ids)

  subnet_id = data.aws_subnet.eks_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}
{% endif %}