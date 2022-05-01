data "aws_ami" "debian" {
  most_recent = true

  filter {
    name   = "name"
    values = [var.ec2_image_info.name]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }

  # to get owner id:
  # aws ec2 describe-images --image-ids <ami-id> --region us-west-2 | jq -r '.Images[0].OwnerId'
  owners = [var.ec2_image_info.owners]
}

resource "aws_instance" "ec2_instance" {
  ami           = data.aws_ami.debian.id
  instance_type = var.ec2_instance.instance_type

  # disk
  root_block_device {
    volume_size = "30" # GiB
    volume_type = "gp2"
    encrypted = true
  }

  # network
  associate_public_ip_address = true

  # security
  vpc_security_group_ids = [aws_security_group.ec2_instance.id]
  subnet_id = aws_subnet.ec2_zone_a[0].id
  security_groups = [aws_security_group.ec2_instance.id]

  user_data = local.bootstrap
  user_data_replace_on_change = false

  tags = merge(
      local.tags_common,
      {
        "Service" = "EC2"
      }
    )

  depends_on = [
    aws_s3_bucket.kubeconfigs_bucket
  ]
}

resource "time_static" "on_ec2_create" {}

locals {
  bootstrap = <<BOOTSTRAP
#!/bin/bash

export KUBECONFIG_FILENAME="${var.kubernetes_cluster_id}.yaml"
export KUBECONFIG_PATH="/tmp/$KUBECONFIG_FILENAME"

apt-get update
apt-get -y install curl s3cmd

export INSTALL_K3S_VERSION=${var.k3s_config.version}
export INSTALL_K3S_CHANNEL=${var.k3s_config.channel}
export INSTALL_K3S_EXEC="--https-listen-port=${random_integer.kubernetes_external_port.result} ${var.k3s_config.exec}"
curl -sfL https://get.k3s.io | sh -
echo 'export KUBECONFIG=/etc/rancher/k3s/k3s.yaml' >> /etc/profile

while [ ! -f /etc/rancher/k3s/k3s.yaml ] ; do
    echo "kubeconfig is not yet present, sleeping"
    sleep 1
done

# Calico will be installed and metadata won't be accessible anymore, it can only be done during bootstrap
sed -r "s/127.0.0.1:6443/$(curl -s http://169.254.169.254/latest/meta-data/public-hostname):${random_integer.kubernetes_external_port.result}/g" /etc/rancher/k3s/k3s.yaml > $KUBECONFIG_PATH
s3cmd --access_key={{ aws_access_key }} --secret_key={{ aws_secret_key }} --region={{ aws_region }} put $KUBECONFIG_PATH s3://${var.s3_bucket_kubeconfig}/$KUBECONFIG_FILENAME
rm -f $KUBECONFIG_PATH
BOOTSTRAP
}