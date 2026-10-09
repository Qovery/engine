use qovery_engine::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use qovery_engine::tera_utils::render_one_off;
use serde_json::{Value, json};
use tera::Context;

const EKS_CLUSTER_TEMPLATE: &str = include_str!("../lib/aws/bootstrap/terraform/eks-master-cluster.j2.tf");

fn rendered_log_types(input: Value) -> Value {
    let settings: ClusterAdvancedSettings = serde_json::from_value(input).unwrap();
    let mut context = Context::from_serialize(&settings).unwrap();
    context.insert("resource_tags", &json!({}));
    context.insert("aws_apn_id", "test");
    context.insert("resource_expiration_in_seconds", &-1);
    context.insert("public_access_cidrs", &vec!["0.0.0.0/0"]);
    context.insert("user_provided_network", &false);
    context.insert("vpc_qovery_network_mode", "WithoutNatGateways");

    let rendered = render_one_off(EKS_CLUSTER_TEMPLATE, &context).unwrap();
    let log_types = rendered
        .lines()
        .find_map(|line| line.trim().strip_prefix("enabled_cluster_log_types = "))
        .expect("EKS cluster must configure its log types");
    serde_json::from_str(log_types).unwrap()
}

#[test]
fn eks_log_types_default_to_all_current_types() {
    assert_eq!(
        rendered_log_types(json!({})),
        json!(["api", "audit", "authenticator", "controllerManager", "scheduler"]),
    );
}

#[test]
fn eks_log_types_render_only_selected_types() {
    assert_eq!(
        rendered_log_types(json!({"aws.cloudwatch.eks_logs_types": ["audit", "controllerManager"]})),
        json!(["audit", "controllerManager"]),
    );
}

#[test]
fn eks_log_types_empty_list_disables_exports() {
    assert_eq!(rendered_log_types(json!({"aws.cloudwatch.eks_logs_types": []})), json!([]));
}

#[test]
fn eks_log_types_reject_invalid_values() {
    for log_types in [
        json!(["unsupported"]),
        json!(["controller-manager"]),
        json!(["api\"]\n  injected = true\n#"]),
        json!("audit"),
        json!(null),
    ] {
        assert!(
            serde_json::from_value::<ClusterAdvancedSettings>(json!({"aws.cloudwatch.eks_logs_types": log_types}))
                .is_err(),
        );
    }
}
