use qovery_engine::io_models::models::VpcQoveryNetworkMode;
use tera::{Context, Tera};

const EKS_VPC_ENDPOINTS_TEMPLATE: &str = include_str!("../lib/aws/bootstrap/terraform/eks-vpc-endpoints.j2.tf");
const EKS_ECR_PULL_THROUGH_CACHE_IAM_TEMPLATE: &str =
    include_str!("../lib/aws/bootstrap/terraform/eks-ecr-pull-through-cache-iam.j2.tf");

fn render_vpc_endpoints_template(pull_through_cache_enabled: bool) -> String {
    let mut context = Context::new();
    context.insert("aws_ecr_enable_pull_through_cache", &pull_through_cache_enabled);
    context.insert("user_provided_network", &false);
    context.insert("vpc_qovery_network_mode", &VpcQoveryNetworkMode::WithNatGateways.to_string());

    Tera::one_off(EKS_VPC_ENDPOINTS_TEMPLATE, &context, false).expect("ECR VPC endpoints template should render")
}

fn render_iam_template(pull_through_cache_enabled: bool) -> String {
    let mut context = Context::new();
    context.insert("aws_ecr_enable_pull_through_cache", &pull_through_cache_enabled);

    Tera::one_off(EKS_ECR_PULL_THROUGH_CACHE_IAM_TEMPLATE, &context, false)
        .expect("ECR pull through cache IAM template should render")
}

#[test]
fn enabled_cache_renders_ecr_endpoints_and_node_permissions() {
    let rendered = render_vpc_endpoints_template(true);
    let rendered_iam = render_iam_template(true);

    assert!(rendered.contains("resource \"aws_vpc_endpoint\" \"ecr_api\""));
    assert!(rendered.contains("resource \"aws_vpc_endpoint\" \"ecr_dkr\""));
    assert!(rendered.contains("com.amazonaws.${var.region}.ecr.api"));
    assert!(rendered.contains("com.amazonaws.${var.region}.ecr.dkr"));
    assert!(rendered_iam.contains("resource \"aws_iam_role_policy\" \"eks_workers_ecr_pull_through_cache\""));
    assert!(rendered_iam.contains("resource \"aws_iam_role_policy\" \"karpenter_nodes_ecr_pull_through_cache\""));
    assert!(rendered_iam.contains("ecr:BatchImportUpstreamImage"));
    assert!(rendered_iam.contains("ecr:CreateRepository"));
    assert!(rendered_iam.contains("repository/qovery-ecr-public/*"));
}

#[test]
fn disabled_cache_does_not_render_ecr_endpoints_or_node_permissions() {
    let rendered = render_vpc_endpoints_template(false);
    let rendered_iam = render_iam_template(false);

    assert!(!rendered.contains("aws_vpc_endpoint\" \"ecr_api"));
    assert!(!rendered.contains("aws_vpc_endpoint\" \"ecr_dkr"));
    assert!(!rendered_iam.contains("ecr:BatchImportUpstreamImage"));
    assert!(!rendered_iam.contains("ecr:CreateRepository"));
}
