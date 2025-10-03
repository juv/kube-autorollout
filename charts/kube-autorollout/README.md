# kube-autorollout

![Version: 0.1.0](https://img.shields.io/badge/Version-0.1.0-informational?style=flat-square) ![Type: application](https://img.shields.io/badge/Type-application-informational?style=flat-square) ![AppVersion: 0.1.0](https://img.shields.io/badge/AppVersion-0.1.0-informational?style=flat-square)

A Helm chart for kube-autorollout

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| affinity | object | `{}` | Affinity configuration for the kube-autorollout controller. More information can be found here: https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/#affinity-and-anti-affinity |
| config.cronSchedule | string | `"*/15 * * * * *"` | The cron schedule to execute the main controller code, given in a format supported by Croner: https://github.com/Hexagon/croner-rust?tab=readme-ov-file#pattern. Example: "*/15 * * * * *" to execute every 15 seconds |
| config.featureFlags.enableJfrogArtifactoryFallback | bool | `false` | Enable JFrog Artifactory fallback when the Artifactory is configured to use the Repository Path Method (https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker) |
| config.registries | list | `[{"hostnamePattern":null,"secret":{"key":null,"name":null},"token":null,"username":null}]` | Container registries |
| config.registries[0].secret | object | `{"key":null,"name":null}` | The Kubernetes secret to mount as an environment variable into the pod |
| config.registries[0].secret.key | string | `nil` | The key to reference of the secret. Will be referenced in the config automatically if .token is unset |
| config.registries[0].secret.name | string | `nil` | Kubernetes Secret name to reference that contains the Docker Registry API token, personal access token, JFrog Artifactory identity token, etc. |
| config.registries[0].token | string | `nil` | Not recommended for production use - use .secret instead. A hardcoded token (api token, personal access token, etc.) to be passed in the Authorization header of the Docker manifest request to the registry |
| config.registries[0].username | string | `nil` | Optional. The username to use for this registry. Only used when the registry is found to be requiring an advanced token flow for authentication, that involves trading in the username and api key / api token into a short-living OAuth2.0-esque access token. This is required for ghcr.io and docker.io |
| config.tls | object | `{"caCertificatePaths":[]}` | TLS configuration |
| config.tls.caCertificatePaths | list | `[]` | Custom CA certificate paths within the container |
| config.webserver | object | `{"port":8080}` | Webserver configuration |
| config.webserver.port | int | `8080` | Webserver port |
| fullnameOverride | string | `""` | String to fully override `"kube-autorollout.fullname"` |
| image.pullPolicy | string | `"IfNotPresent"` | Image pull policy for the container image |
| image.repository | string | `"ghcr.io/juv/kube-autorollout"` | The image repository name to use for the container image |
| image.tag | string | `"v0.1.0"` | Image tag to use for the container image. Overrides the image tag whose default is the chart appVersion. |
| imagePullSecrets | list | `[]` | Secrets with credentials to pull images from a private registry. More information can be found here: https://kubernetes.io/docs/tasks/configure-pod-container/pull-image-private-registry/ |
| livenessProbe | object | `{"httpGet":{"path":"/health/live","port":"http"}}` | Liveness probe for the kube-autorollout controller. More information can be found here: https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/ |
| nameOverride | string | `""` | Override to the chart name |
| nodeSelector | object | `{}` | Node selector for the kube-autorollout controller. More information can be found here: https://kubernetes.io/docs/concepts/scheduling-eviction/assign-pod-node/#nodeselector |
| podAnnotations | object | `{}` | Pod annotations for kube-autorollout. More information can be found here: https://kubernetes.io/docs/concepts/overview/working-with-objects/annotations/ |
| podLabels | object | `{}` | Pod labels for kube-autorollout. More information can be found here: https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/ |
| podSecurityContext | object | `{}` | kube-autorollout pod-level security context. More information can be found here: https://kubernetes.io/docs/tasks/configure-pod-container/security-context/ |
| rbac | object | `{"enabled":true}` | Kubernetes RBAC configuration |
| rbac.enabled | bool | `true` | Switch to enable/disable the creation of Kubernetes role and rolebinding for the kube-autorollout service account automatically. If false, the role and rolebinding that targets the service account must be created separately. |
| readinessProbe | object | `{"httpGet":{"path":"/health/ready","port":"http"}}` | Readiness probe for kube-autorollout controller. More information can be found here: https://kubernetes.io/docs/tasks/configure-pod-container/configure-liveness-readiness-startup-probes/ |
| replicaCount | int | `1` | The number of application controller pods to run. A number higher than one does not make sense at this time as the controller is not supporting sharding. |
| resources | object | `{}` | Resource requests and limits for the kube-autorollout pod |
| securityContext | object | `{}` | kube-autorollout container-level security context. More information can be found here: https://kubernetes.io/docs/tasks/configure-pod-container/security-context/ |
| serviceAccount.annotations | object | `{}` | Annotations to add to the service account |
| serviceAccount.automount | bool | `true` | Automatically mount a ServiceAccount's API credentials? |
| serviceAccount.create | bool | `true` | Specifies whether a service account should be created |
| serviceAccount.name | string | `""` | The name of the service account to use. If not set and create is true, a name is generated using the fullname template |
| tolerations | list | `[]` | Tolerations for the kube-autorollout controller. More information can be found here: https://kubernetes.io/docs/concepts/scheduling-eviction/taint-and-toleration/ |

----------------------------------------------
Autogenerated from chart metadata using [helm-docs v1.14.2](https://github.com/norwoodj/helm-docs/releases/v1.14.2)
