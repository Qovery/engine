/**
 * Copyright 2022 Google LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

// This file was automatically generated from a template in ./autogen/main

data "google_compute_subnetwork" "gke_subnetwork" {
  provider = google

  count   = var.add_cluster_firewall_rules ? 1 : 0
  name    = var.subnetwork
  region  = var.region
  project = local.network_project_id
}

{% if vpc_use_existing %}
data "google_compute_network" "vpc_network" {
  name = var.vpc_name
}
{% else %}
resource "google_compute_network" "vpc_network" {
  project                 = var.project_id
  name                    = var.vpc_name
  auto_create_subnetworks = var.auto_create_subnetworks
  # Putting tags as JSON in description since VPC don't support tags
  description             = jsonencode(local.tags_common) # limited length to 2048 chars
  # ignore changes in description since it's not supposed to change and because it creates an issue when network is destroying,
  # there is an open bug on GCP side => https://issuetracker.google.com/issues/186792016
  lifecycle {
    ignore_changes = [description]
  }
}

# Activate / Deactivate VPC logs flow via gcloud CLI
# This is a workaround to enable / disable VPC flow logs since it's not supported by Terraform
# TODO(benjaminch): Remove this workaround once Terraform supports it OR once the Google Rust API supports it
data "google_compute_network" "vpc" {
  name = google_compute_network.vpc_network.name

  depends_on = [
    google_compute_network.vpc_network
  ]
}

# This is a dirty workaround allowing to get subnetworks self links via data after network creation
# `for_each` doesn't seem to like the when using not yet created resources
locals {
    vpc_logs_flow_gcloud_commands = [for i, subnetwork in data.google_compute_network.vpc.subnetworks_self_links : "gcloud compute networks subnets update ${subnetwork} {% if vpc_enable_flow_logs %} --enable-flow-logs --logging-flow-sampling=${var.vpc_flow_logs_sampling} {% else %} --no-enable-flow-logs {% endif %} --project=${var.project_id}"]
}

resource "null_resource" "set_subnetwork_vpc_log_flow" {
  triggers = {
    always_run = "${timestamp()}"
  }

  provisioner "local-exec" {
    command = join(" && ", local.vpc_logs_flow_gcloud_commands)
  }

  depends_on = [
    data.google_compute_network.vpc
  ]
}

{% endif %}