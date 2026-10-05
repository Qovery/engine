//! Covers the HTTP-to-HTTPS redirect semantics emitted by the Gateway API route template.

const HTTP_ROUTE_TEMPLATE: &str = include_str!("../lib/common/charts/q-ingress-tls/templates/gateway-http-route.yaml");

#[test]
fn ssl_redirect_preserves_the_request_method() {
    assert!(
        HTTP_ROUTE_TEMPLATE.contains("requestRedirect:\n          scheme: https\n          statusCode: 308"),
        "a permanent redirect must preserve POST and other non-GET methods"
    );
}
