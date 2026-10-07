use qovery_engine::tera_utils::render_one_off;
use tera::Context;

const EKS_MASTER_IAM_TEMPLATE: &str = include_str!("../lib/aws/bootstrap/terraform/eks-master-iam.j2.tf");
const AWS_PROVIDERS_TEMPLATE: &str = include_str!("../lib/aws/bootstrap/terraform/tf-providers-aws.j2.tf");

fn context(wide_permissions_enabled: bool) -> Context {
    let mut context = Context::new();
    context.insert("aws_iam_user_mapper_sso_enabled", &false);
    context.insert("aws_region", "eu-west-3");
    context.insert("aws_access_key", "AKIA_CURRENT");
    context.insert("aws_secret_key", "CURRENT_SECRET");
    context.insert("aws_session_token", &Some("CURRENT_TOKEN"));
    context.insert("aws_wide_permissions_enabled", &wide_permissions_enabled);
    if wide_permissions_enabled {
        context.insert("aws_wide_permissions_options_access_key", "AKIA_OPTIONS");
        context.insert("aws_wide_permissions_options_secret_key", "OPTIONS_SECRET");
        context.insert("aws_wide_permissions_options_session_token", "OPTIONS_TOKEN");
    } else {
        context.insert("aws_wide_permissions_options_access_key", "");
        context.insert("aws_wide_permissions_options_secret_key", "");
        context.insert("aws_wide_permissions_options_session_token", "");
    }
    context
}

fn render(template: &str, wide_permissions_enabled: bool) -> String {
    render_one_off(template, &context(wide_permissions_enabled)).expect("template should render")
}

#[test]
fn without_wide_permissions_credentials_access_is_given_to_the_current_identity() {
    let providers = render(AWS_PROVIDERS_TEMPLATE, false);
    let iam = render(EKS_MASTER_IAM_TEMPLATE, false);

    assert_eq!(providers.matches("provider \"aws\" {").count(), 1);
    assert!(!providers.contains("options_credentials"));

    assert!(iam.contains("options_caller_arn = data.aws_caller_identity.current.arn"));
    assert!(!iam.contains("data \"aws_caller_identity\" \"wide_permissions_options_credentials\""));
    assert!(!iam.contains("qovery_eks_access_wide_permissions"));
    assert!(iam.contains("resource \"aws_eks_access_entry\" \"qovery_eks_access\" {"));
    assert!(iam.contains("resource \"aws_eks_access_policy_association\" \"qovery_eks_access\" {"));
}

#[test]
fn with_wide_permissions_credentials_access_is_given_to_both_identities() {
    let providers = render(AWS_PROVIDERS_TEMPLATE, true);
    let iam = render(EKS_MASTER_IAM_TEMPLATE, true);

    // Default provider uses the current (wide permissions) credentials, the aliased one the options credentials
    assert_eq!(providers.matches("provider \"aws\" {").count(), 2);
    assert!(providers.contains("access_key = \"AKIA_CURRENT\""));
    assert!(providers.contains("alias      = \"wide_permissions_options_credentials\""));
    assert!(providers.contains("access_key = \"AKIA_OPTIONS\""));
    assert!(providers.contains("secret_key = \"OPTIONS_SECRET\""));
    assert!(providers.contains("token      = \"OPTIONS_TOKEN\""));

    // The options identity keeps the historical access entry, so existing states are left untouched
    assert!(iam.contains("provider = aws.wide_permissions_options_credentials"));
    assert!(iam.contains("options_caller_arn = data.aws_caller_identity.wide_permissions_options_credentials.arn"));
    assert!(iam.contains("wide_permissions_caller_arn = data.aws_caller_identity.current.arn"));
    assert!(iam.contains("resource \"aws_eks_access_entry\" \"qovery_eks_access\" {"));
    assert!(iam.contains("resource \"aws_eks_access_entry\" \"qovery_eks_access_wide_permissions\" {"));
    assert!(iam.contains("resource \"aws_eks_access_policy_association\" \"qovery_eks_access_wide_permissions\" {"));
    assert_eq!(
        iam.matches("count             = local.create_wide_permissions_access ? 1 : 0")
            .count(),
        2
    );
}
