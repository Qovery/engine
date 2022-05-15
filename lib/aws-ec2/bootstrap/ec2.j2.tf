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

resource "aws_key_pair" "qovery_ssh_key" {
  key_name   = "qovery-key"
  public_key = "{{ qovery_ssh_key }}"
}

resource "aws_instance" "ec2_instance" {
  ami           = data.aws_ami.debian.id
  instance_type = var.ec2_instance.instance_type

  # root disk
  root_block_device {
    volume_size = var.ec2_instance.disk_size_in_gb # GiB
    volume_type = "gp2"
    encrypted = true
    delete_on_termination = false
  }

  # network
  associate_public_ip_address = true

  # security
  vpc_security_group_ids = [aws_security_group.ec2_instance.id]
  subnet_id = aws_subnet.ec2_zone_a[0].id

  # ssh
  key_name = aws_key_pair.qovery_ssh_key.key_name

  # k3s install
  user_data = local.bootstrap
  user_data_replace_on_change = true

  tags = merge(
      local.tags_common,
      {
        "Service" = "EC2"
      }
    )

  depends_on = [
    aws_s3_bucket.kubeconfigs_bucket,
  ]
}

resource "time_static" "on_ec2_create" {}

locals {
  bootstrap = <<BOOTSTRAP
#!/bin/bash

function print_title() {
  echo -e "\n######### $1 #########\n"
}

# enable logs to file and console
exec > >(tee ${var.ec2_instance.user_data_logs_path}|logger -t user_data -s 2>/dev/console) 2>&1

print_title "Install packages"
apt-get update
apt-get -y install curl s3cmd parted

print_title "Install k3s"
export INSTALL_K3S_VERSION=${var.k3s_config.version}
export INSTALL_K3S_CHANNEL=${var.k3s_config.channel}
export INSTALL_K3S_EXEC="--https-listen-port=${random_integer.kubernetes_external_port.result} ${var.k3s_config.exec}"
echo "k3s agrs: $INSTALL_K3S_EXEC"
curl -fL https://get.k3s.io | sh -
echo 'export KUBECONFIG=/etc/rancher/k3s/k3s.yaml' >> /etc/profile

print_title "Create Qovery boot script"
mkdir /etc/qovery
cat << "EOF" > /etc/qovery/boot.sh
#!/bin/bash

KUBECONFIG_FILENAME="${var.kubernetes_cluster_id}.yaml"
NEW_KUBECONFIG_PATH="/tmp/$KUBECONFIG_FILENAME"

while [ ! -f /etc/rancher/k3s/k3s.yaml ] ; do
    echo "kubeconfig is not yet present, sleeping"
    sleep 1
done

# Calico will be installed and metadata won't be accessible anymore, it can only be done during bootstrap
public_hostname="$(curl -s http://169.254.169.254/latest/meta-data/public-hostname)"
sed "s/127.0.0.1/$public_hostname/g" /etc/rancher/k3s/k3s.yaml > $NEW_KUBECONFIG_PATH
sed -i "s/:6443/:${random_integer.kubernetes_external_port.result}/g" $NEW_KUBECONFIG_PATH
s3cmd --access_key={{ aws_access_key }} --secret_key={{ aws_secret_key }} --region={{ aws_region }} put $NEW_KUBECONFIG_PATH s3://${var.s3_bucket_kubeconfig}/$KUBECONFIG_FILENAME
rm -f $NEW_KUBECONFIG_PATH
EOF

print_title "Create Qovery systemd boot service"
cat << EOF > /etc/systemd/system/qovery-boot.service
[Unit]
Description=Qovery boot service

[Service]
ExecStart=/etc/qovery/boot.sh

[Install]
WantedBy=multi-user.target
EOF

print_title "Set permissions and start service"
chmod -R 700 /etc/qovery
chmod 755 /etc/systemd/system/qovery-boot.service
systemctl enable qovery-boot.service
systemctl start qovery-boot.service

BOOTSTRAP
}