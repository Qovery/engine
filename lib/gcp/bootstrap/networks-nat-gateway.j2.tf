{% if vpc_network_mode == "WithNatGateways" and not vpc_use_existing %}

resource "google_compute_router" "router" {
  project     = var.project_id
  name        = var.vpc_name
  network     = google_compute_network.vpc_network.id
  region      = var.region
  bgp {
    asn = 64514
  }

  description = jsonencode(local.tags_common) # limited length to 2048 chars

  lifecycle {
    ignore_changes = [description]
  }
}

resource "google_compute_route" "gke-master-default-gw" {
  count            = 1
  # count = google_container_cluster.primary.endpoint == "" ? 0 : length(split(";", google_container_cluster.primary.endpoint))
  name             = "${var.vpc_name}-master-default-gw-${count.index + 1}"
  dest_range       = "${element(split(";", replace(google_container_cluster.primary.endpoint, "/32", "")), count.index)}"
  network          = google_compute_network.vpc_network.id
  next_hop_gateway = "default-internet-gateway"
  priority         = 700

  depends_on = [
    google_container_cluster.primary,
  ]
}

resource "google_compute_router_nat" "nat" {
  name                               = google_compute_router.router.name
  router                             = google_compute_router.router.name
  region                             = google_compute_router.router.region
  nat_ip_allocate_option             = "AUTO_ONLY"
  source_subnetwork_ip_ranges_to_nat = "ALL_SUBNETWORKS_ALL_IP_RANGES"

  log_config {
    enable = false
    filter = "ERRORS_ONLY"
  }
}

{% endif %}