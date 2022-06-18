#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoLoadBalancers {
    pub api_version: String,
    pub items: Vec<DoLoadBalancer>,
    pub kind: String,
    pub metadata: Metadata2,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoLoadBalancer {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: Spec,
    pub status: Status,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub annotations: Annotations,
    pub creation_timestamp: String,
    pub finalizers: Vec<String>,
    pub name: String,
    pub namespace: String,
    pub resource_version: String,
    pub uid: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotations {
    #[serde(rename = "kubernetes.digitalocean.com/load-balancer-id")]
    pub kubernetes_digitalocean_com_load_balancer_id: String,
    #[serde(rename = "meta.helm.sh/release-name")]
    pub meta_helm_sh_release_name: String,
    #[serde(rename = "meta.helm.sh/release-namespace")]
    pub meta_helm_sh_release_namespace: String,
    #[serde(rename = "service.beta.kubernetes.io/do-loadbalancer-algorithm")]
    pub service_beta_kubernetes_io_do_loadbalancer_algorithm: String,
    #[serde(rename = "service.beta.kubernetes.io/do-loadbalancer-enable-proxy-protocol")]
    pub service_beta_kubernetes_io_do_loadbalancer_enable_proxy_protocol: String,
    #[serde(rename = "service.beta.kubernetes.io/do-loadbalancer-hostname")]
    pub service_beta_kubernetes_io_do_loadbalancer_hostname: Option<String>,
    #[serde(rename = "service.beta.kubernetes.io/do-loadbalancer-name")]
    pub service_beta_kubernetes_io_do_loadbalancer_name: String,
    #[serde(rename = "service.beta.kubernetes.io/do-loadbalancer-protocol")]
    pub service_beta_kubernetes_io_do_loadbalancer_protocol: String,
    #[serde(rename = "service.beta.kubernetes.io/do-loadbalancer-size-slug")]
    pub service_beta_kubernetes_io_do_loadbalancer_size_slug: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedField {
    pub api_version: String,
    pub fields_type: String,
    pub fields_v1: FieldsV1,
    pub manager: String,
    pub operation: String,
    pub time: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldsV1 {
    #[serde(rename = "f:metadata")]
    pub f_metadata: FMetadata,
    #[serde(rename = "f:spec")]
    pub f_spec: Option<FSpec>,
    #[serde(rename = "f:status")]
    pub f_status: Option<FStatus>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FMetadata {
    #[serde(rename = "f:annotations")]
    pub f_annotations: FAnnotations,
    #[serde(rename = "f:labels")]
    pub f_labels: Option<FLabels>,
    #[serde(rename = "f:finalizers")]
    pub f_finalizers: Option<FFinalizers>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FAnnotations {
    #[serde(rename = ".")]
    pub field: Option<GeneratedType>,
    #[serde(rename = "f:meta.helm.sh/release-name")]
    pub f_meta_helm_sh_release_name: Option<FMetaHelmShReleaseName>,
    #[serde(rename = "f:meta.helm.sh/release-namespace")]
    pub f_meta_helm_sh_release_namespace: Option<FMetaHelmShReleaseNamespace>,
    #[serde(rename = "f:service.beta.kubernetes.io/do-loadbalancer-enable-proxy-protocol")]
    pub f_service_beta_kubernetes_io_do_loadbalancer_enable_proxy_protocol:
        Option<FServiceBetaKubernetesIoDoLoadbalancerEnableProxyProtocol>,
    #[serde(rename = "f:kubernetes.digitalocean.com/load-balancer-id")]
    pub f_kubernetes_digitalocean_com_load_balancer_id: Option<FKubernetesDigitaloceanComLoadBalancerId>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedType {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FMetaHelmShReleaseName {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FMetaHelmShReleaseNamespace {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FServiceBetaKubernetesIoDoLoadbalancerEnableProxyProtocol {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FKubernetesDigitaloceanComLoadBalancerId {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FLabels {
    #[serde(rename = ".")]
    pub field: GeneratedType2,
    #[serde(rename = "f:app")]
    pub f_app: FApp,
    #[serde(rename = "f:app.kubernetes.io/managed-by")]
    pub f_app_kubernetes_io_managed_by: FAppKubernetesIoManagedBy,
    #[serde(rename = "f:chart")]
    pub f_chart: FChart,
    #[serde(rename = "f:component")]
    pub f_component: FComponent,
    #[serde(rename = "f:heritage")]
    pub f_heritage: FHeritage,
    #[serde(rename = "f:release")]
    pub f_release: FRelease,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedType2 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FApp {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FAppKubernetesIoManagedBy {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FChart {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FComponent {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FHeritage {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FRelease {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FFinalizers {
    #[serde(rename = ".")]
    pub field: GeneratedType3,
    #[serde(rename = "v:\"service.kubernetes.io/load-balancer-cleanup\"")]
    pub v_service_kubernetes_io_load_balancer_cleanup: VServiceKubernetesIoLoadBalancerCleanup,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedType3 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VServiceKubernetesIoLoadBalancerCleanup {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FSpec {
    #[serde(rename = "f:externalTrafficPolicy")]
    pub f_external_traffic_policy: FExternalTrafficPolicy,
    #[serde(rename = "f:ports")]
    pub f_ports: FPorts,
    #[serde(rename = "f:selector")]
    pub f_selector: FSelector,
    #[serde(rename = "f:sessionAffinity")]
    pub f_session_affinity: FSessionAffinity,
    #[serde(rename = "f:type")]
    pub f_type: FType,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FExternalTrafficPolicy {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FPorts {
    #[serde(rename = ".")]
    pub field: GeneratedType4,
    #[serde(rename = "k:{\"port\":443,\"protocol\":\"TCP\"}")]
    pub k_port443_protocol_tcp: KPort443ProtocolTcp,
    #[serde(rename = "k:{\"port\":80,\"protocol\":\"TCP\"}")]
    pub k_port80_protocol_tcp: KPort80ProtocolTcp,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedType4 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KPort443ProtocolTcp {
    #[serde(rename = ".")]
    pub field: GeneratedType5,
    #[serde(rename = "f:name")]
    pub f_name: FName,
    #[serde(rename = "f:port")]
    pub f_port: FPort,
    #[serde(rename = "f:protocol")]
    pub f_protocol: FProtocol,
    #[serde(rename = "f:targetPort")]
    pub f_target_port: FTargetPort,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedType5 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FName {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FPort {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FProtocol {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FTargetPort {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KPort80ProtocolTcp {
    #[serde(rename = ".")]
    pub field: GeneratedType6,
    #[serde(rename = "f:name")]
    pub f_name: FName2,
    #[serde(rename = "f:port")]
    pub f_port: FPort2,
    #[serde(rename = "f:protocol")]
    pub f_protocol: FProtocol2,
    #[serde(rename = "f:targetPort")]
    pub f_target_port: FTargetPort2,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedType6 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FName2 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FPort2 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FProtocol2 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FTargetPort2 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FSelector {
    #[serde(rename = ".")]
    pub field: GeneratedType7,
    #[serde(rename = "f:app")]
    pub f_app: FApp2,
    #[serde(rename = "f:app.kubernetes.io/component")]
    pub f_app_kubernetes_io_component: FAppKubernetesIoComponent,
    #[serde(rename = "f:release")]
    pub f_release: FRelease2,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedType7 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FApp2 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FAppKubernetesIoComponent {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FRelease2 {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FSessionAffinity {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FType {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FStatus {
    #[serde(rename = "f:loadBalancer")]
    pub f_load_balancer: FLoadBalancer,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FLoadBalancer {
    #[serde(rename = "f:ingress")]
    pub f_ingress: FIngress,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FIngress {}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    #[serde(rename = "clusterIP")]
    pub cluster_ip: String,
    pub external_traffic_policy: String,
    pub health_check_node_port: i64,
    pub ports: Vec<Port>,
    pub session_affinity: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Port {
    pub name: String,
    pub node_port: i64,
    pub port: i64,
    pub protocol: String,
    pub target_port: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub load_balancer: LoadBalancer,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancer {
    pub ingress: Vec<Ingress>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ingress {
    #[serde(alias = "hostname")]
    pub ip: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata2 {
    pub resource_version: String,
    #[serde(default)]
    pub self_link: String,
}
