{% if vpc_qovery_network_mode == "WithNatGateways" and not user_provided_network %}

variable "eks_subnets_zone_a_private" {
  description = "EKS private subnets Zone A"
  default = {{ eks_zone_a_subnet_blocks_private }}
  type = list(string)
}

variable "eks_subnets_zone_b_private" {
  description = "EKS private subnets Zone B"
  default = {{ eks_zone_b_subnet_blocks_private }}
  type = list(string)
}

variable "eks_subnets_zone_c_private" {
  description = "EKS private subnets Zone C"
  default = {{eks_zone_c_subnet_blocks_private}}
  type = list(string)
}


variable "eks_subnets_zone_a_public" {
  description = "EKS public subnets Zone A"
  default = {{ eks_zone_a_subnet_blocks_public }}
  type = list(string)
}

variable "eks_subnets_zone_b_public" {
  description = "EKS public subnets Zone B"
  default = {{ eks_zone_b_subnet_blocks_public }}
  type = list(string)
}

variable "eks_subnets_zone_c_public" {
  description = "EKS public subnets Zone C"
  default = {{ eks_zone_c_subnet_blocks_public }}
  type = list(string)
}

# External IPs
resource "aws_eip" "eip_zone_a" {
  domain = "vpc"
  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-eip-${var.aws_availability_zones[0]}"
    }
  )
}

resource "aws_eip" "eip_zone_b" {
  domain = "vpc"
  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-eip-${var.aws_availability_zones[1]}"
    }
  )
}

resource "aws_eip" "eip_zone_c" {
  domain = "vpc"
  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-eip-${var.aws_availability_zones[2]}"
    }
  )
}

{% if aws_enable_nat_gateway_secondary_eip %}
# Secondary External IPs for NAT Gateways
resource "aws_eip" "eip_secondary_zone_a" {
  domain = "vpc"
  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-eip-secondary-${var.aws_availability_zones[0]}"
    }
  )
}

resource "aws_eip" "eip_secondary_zone_b" {
  domain = "vpc"
  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-eip-secondary-${var.aws_availability_zones[1]}"
    }
  )
}

resource "aws_eip" "eip_secondary_zone_c" {
  domain = "vpc"
  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-eip-secondary-${var.aws_availability_zones[2]}"
    }
  )
}
{% endif %}

# Public subnets
resource "aws_subnet" "eks_zone_a_public" {
  count = length(var.eks_subnets_zone_a_public)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.eks_subnets_zone_a_public[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = merge(
    local.tags_eks_vpc_public,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-public-${var.aws_availability_zones[0]}"
    }
  )
}

resource "aws_subnet" "eks_zone_b_public" {
  count = length(var.eks_subnets_zone_b_public)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.eks_subnets_zone_b_public[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = merge(
    local.tags_eks_vpc_public,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-public-${var.aws_availability_zones[1]}"
    }
  )
}

resource "aws_subnet" "eks_zone_c_public" {
  count = length(var.eks_subnets_zone_c_public)

  availability_zone = var.aws_availability_zones[2]
  cidr_block = var.eks_subnets_zone_c_public[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true

  tags = merge(
    local.tags_eks_vpc_public,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-public-${var.aws_availability_zones[2]}"
    }
  )
}

# Public Nat gateways
resource "aws_nat_gateway" "eks_zone_a_public" {
  count = length(var.eks_subnets_zone_a_public)

  allocation_id = aws_eip.eip_zone_a.id
{% if aws_enable_nat_gateway_secondary_eip %}
  secondary_allocation_ids = [aws_eip.eip_secondary_zone_a.id]
{% endif %}
  subnet_id     = aws_subnet.eks_zone_a_public[count.index].id

  tags = merge(
    local.tags_eks_vpc_public,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-${var.aws_availability_zones[0]}"
    }
  )
}

resource "aws_nat_gateway" "eks_zone_b_public" {
  count = length(var.eks_subnets_zone_b_public)

  allocation_id = aws_eip.eip_zone_b.id
{% if aws_enable_nat_gateway_secondary_eip %}
  secondary_allocation_ids = [aws_eip.eip_secondary_zone_b.id]
{% endif %}
  subnet_id     = aws_subnet.eks_zone_b_public[count.index].id

  tags = merge(
    local.tags_eks_vpc_public,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-${var.aws_availability_zones[1]}"
    }
  )
}

resource "aws_nat_gateway" "eks_zone_c_public" {
  count = length(var.eks_subnets_zone_c_public)

  allocation_id = aws_eip.eip_zone_c.id
{% if aws_enable_nat_gateway_secondary_eip %}
  secondary_allocation_ids = [aws_eip.eip_secondary_zone_c.id]
{% endif %}
  subnet_id = aws_subnet.eks_zone_c_public[count.index].id

  tags = merge(
    local.tags_eks_vpc_public,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-nat-${var.aws_availability_zones[2]}"
    }
  )
}

# Public Routing table
resource "aws_route_table" "eks_cluster" {
  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.eks_cluster.id
  }

  {% for route in vpc_custom_routing_table %}
  route {
    cidr_block = "{{ route.destination }}"
    gateway_id = "{{ route.target }}"
  }
  {% endfor %}

  tags = merge(
    local.tags_eks_vpc_public,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-rt-public"
    }
  )
}

resource "aws_route_table_association" "eks_cluster_zone_a_public" {
  count = length(var.eks_subnets_zone_a_public)

  subnet_id = aws_subnet.eks_zone_a_public.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_b_public" {
  count = length(var.eks_subnets_zone_b_public)

  subnet_id = aws_subnet.eks_zone_b_public.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster_zone_c_public" {
  count = length(var.eks_subnets_zone_c_public)

  subnet_id = aws_subnet.eks_zone_c_public.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}


# Private subnets
resource "aws_subnet" "eks_zone_a" {
  count = length(var.eks_subnets_zone_a_private)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.eks_subnets_zone_a_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = false

  tags = merge(
    local.tags_eks_vpc_private,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-private-${var.aws_availability_zones[0]}"
      "karpenter.sh/discovery" = var.kubernetes_cluster_name
    }
  )
}

resource "aws_subnet" "eks_zone_b" {
  count = length(var.eks_subnets_zone_b_private)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.eks_subnets_zone_b_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = false

  tags = merge(
    local.tags_eks_vpc_private,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-private-${var.aws_availability_zones[1]}"
      "karpenter.sh/discovery" = var.kubernetes_cluster_name
    }
  )
}

resource "aws_subnet" "eks_zone_c" {
  count = length(var.eks_subnets_zone_c_private)

  availability_zone = var.aws_availability_zones[2]
  cidr_block = var.eks_subnets_zone_c_private[count.index]
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = false

  tags = merge(
    local.tags_eks_vpc_private,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-private-${var.aws_availability_zones[2]}"
      "karpenter.sh/discovery" = var.kubernetes_cluster_name
    }
  )
}

# Routing table
resource "aws_route_table" "eks_cluster_zone_a_private" {
  count = length(aws_nat_gateway.eks_zone_a_public)

  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    nat_gateway_id = aws_nat_gateway.eks_zone_a_public[count.index].id
  }

  {% for route in vpc_custom_routing_table %}
  route {
    cidr_block = "{{ route.destination }}"
    gateway_id = "{{ route.target }}"
  }
  {% endfor %}

  tags = merge(
    local.tags_eks_vpc_private,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-rt-private-${var.aws_availability_zones[0]}"
    }
  )
}

resource "aws_route_table" "eks_cluster_zone_b_private" {
  count = length(aws_nat_gateway.eks_zone_b_public)

  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    nat_gateway_id = aws_nat_gateway.eks_zone_b_public[count.index].id
  }

  {% for route in vpc_custom_routing_table %}
  route {
    cidr_block = "{{ route.destination }}"
    gateway_id = "{{ route.target }}"
  }
  {% endfor %}

  tags = merge(
    local.tags_eks_vpc_private,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-rt-private-${var.aws_availability_zones[1]}"
    }
  )
}

resource "aws_route_table" "eks_cluster_zone_c_private" {
  count = length(aws_nat_gateway.eks_zone_c_public)

  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    nat_gateway_id = aws_nat_gateway.eks_zone_c_public[count.index].id
  }

  {% for route in vpc_custom_routing_table %}
  route {
    cidr_block = "{{ route.destination }}"
    gateway_id = "{{ route.target }}"
  }
  {% endfor %}

  tags = merge(
    local.tags_eks_vpc_private,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-rt-private-${var.aws_availability_zones[2]}"
    }
  )
}

resource "aws_route_table_association" "eks_cluster_zone_a" {
  count = length(var.eks_subnets_zone_a_private)

  subnet_id = aws_subnet.eks_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster_zone_a_private[count.index].id
}

resource "aws_route_table_association" "eks_cluster_zone_b" {
  count = length(var.eks_subnets_zone_b_private)

  subnet_id = aws_subnet.eks_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster_zone_b_private[count.index].id
}

resource "aws_route_table_association" "eks_cluster_zone_c" {
  count = length(var.eks_subnets_zone_c_private)

  subnet_id = aws_subnet.eks_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster_zone_c_private[count.index].id
}
{% endif %}