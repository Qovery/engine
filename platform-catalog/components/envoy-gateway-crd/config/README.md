# envoy-gateway-crd

This bundle installs the Gateway API standard channel, including ListenerSet, and Envoy Gateway
CRDs. It must be deployed before every component that creates Gateway API or Envoy Gateway
resources. The chart is published as `gateway-crds-helm` because that is the frozen upstream
chart identity; the Qovery release and configuration bundle remain `envoy-gateway-crd`.
