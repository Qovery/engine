use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_namespace::NamespaceDeployment;
use crate::environment::models::abort::Abort;
use crate::environment::models::environment::Environment;
use crate::environment::models::router::{HostnameListenerSetPlan, RouterService};
use crate::environment::models::router_listener_set::mapped_hostname_routers;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::io_models::environment::{EnvironmentRoutingMode, HostnameMapping};
use crate::io_models::models::CustomDomain;
use crate::logger::Logger;
use crate::metrics_registry::{StepLabel, StepName, StepStatus};
use crate::runtime::block_on;
use crate::services::aws::load_balancers::clean_up_deleted_k8s_nlb;
use itertools::Itertools;
use k8s_openapi::api::core::v1::Namespace;
use kube::Api;
use std::cmp::{max, min};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::ScopedJoinHandle;
use std::time::Duration;
use uuid::Uuid;

pub struct EnvironmentDeployment<'a> {
    pub deployed_services: Arc<Mutex<HashSet<Uuid>>>,
    pub deployment_target: DeploymentTarget<'a>,
    logger: Arc<Box<dyn Logger>>,
}

impl<'a> EnvironmentDeployment<'a> {
    pub fn new(
        infra_ctx: &'a InfrastructureContext,
        environment: &'a Environment,
        abort: &'a dyn Abort,
        logger: Arc<Box<dyn Logger>>,
    ) -> Result<EnvironmentDeployment<'a>, Box<EngineError>> {
        let deployment_target = DeploymentTarget::new(infra_ctx, environment, abort)?;
        Ok(EnvironmentDeployment {
            deployed_services: Arc::new(Mutex::new(HashSet::with_capacity(
                Self::services_without_routers_iter(environment).count()
                    + Self::services_routers_iter(environment).count(),
            ))),
            deployment_target,
            logger,
        })
    }

    fn services_without_routers_iter(
        environment: &Environment,
    ) -> impl DoubleEndedIterator<Item = (Uuid, &dyn DeploymentAction, Action)> {
        std::iter::empty()
            .chain(
                environment
                    .databases
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .jobs
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .containers
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .applications
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .helm_charts
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .terraform_services
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
            .chain(
                environment
                    .agentic_workflows
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
    }

    fn services_routers_iter(
        environment: &Environment,
    ) -> impl DoubleEndedIterator<Item = (Uuid, &dyn DeploymentAction, Action, Option<Uuid>)> {
        std::iter::empty().chain(
            environment
                .routers
                .iter()
                .map(|r| (*r.long_id(), r.as_deployment_action(), *r.action(), r.associated_service_id())),
        )
    }

    fn should_abort_wrapper<'b>(
        target: &'b DeploymentTarget,
        event_details: &'b EventDetails,
    ) -> impl Fn() -> Result<(), Box<EngineError>> + 'b + Send + Sync {
        move || {
            if target.abort.status().should_cancel() {
                Err(Box::new(EngineError::new_task_cancellation_requested(event_details.clone())))
            } else {
                Ok(())
            }
        }
    }

    pub fn on_create(&mut self) -> Result<(), Box<EngineError>> {
        let target = &self.deployment_target;
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Deploy);
        let resource_expiration = target
            .kubernetes
            .context()
            .resource_expiration_in_seconds()
            .map(|ttl| Duration::from_secs(ttl as u64));
        let metrics_registry = target.metrics_registry.clone();

        let should_abort = Self::should_abort_wrapper(target, &event_details);
        should_abort()?;

        // deploy namespace first
        let ns = NamespaceDeployment {
            resource_expiration,
            event_details: event_details.clone(),
        };
        ns.exec_action(target, target.environment.action)?;

        let services_to_deploy = Self::services_without_routers_iter(target.environment);
        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);
        let deferred_routers = if target.environment.routing_mode == EnvironmentRoutingMode::PerHostnameListenerSet
            && !target.environment.hostname_mapping.is_empty()
        {
            let plans = target
                .environment
                .routers
                .iter()
                .filter(|router| target.environment.hostname_mapping.contains_router(router.long_id()))
                .map(|router| {
                    Ok((
                        *router.long_id(),
                        router.custom_domains(),
                        router.hostname_listener_set_plan(target)?,
                    ))
                })
                .collect::<Result<Vec<_>, Box<EngineError>>>()?;
            let namespaces: Api<Namespace> = Api::all(target.kube.client());
            routers_waiting_for_namespaces(
                &plans,
                &target.environment.hostname_mapping,
                target.environment.namespace(),
                |namespace| {
                    block_on(namespaces.get_opt(namespace))
                        .map(|namespace| namespace.is_some())
                        .map_err(|error| {
                            Box::new(EngineError::new_unknown(
                                event_details.clone(),
                                format!("Cannot check namespace `{namespace}` before creating shared ListenerSets"),
                                Some(CommandError::new_from_safe_message(error.to_string())),
                                None,
                                None,
                            ))
                        })
                },
            )?
        } else {
            HashSet::new()
        };

        self.logger
            .log(proceeding_event(&event_details, parallel_deploys, "deployment(s)"));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        let router_tasks = Self::services_without_routers_iter(target.environment)
            .filter_map(|(service_id, _, _)| {
                Self::get_associated_router(&target.environment.routers, service_id)
                    .filter(|router| deferred_routers.contains(router.long_id()))
            })
            .map(|router| {
                let deployed_services = self.deployed_services.clone();
                move || {
                    deployed_services.lock().unwrap().insert(*router.long_id());
                    router.exec_action(target, *router.action())
                }
            })
            .collect_vec();
        deployment_threads_pool.run_in_phases(
            services_to_deploy
                .into_iter()
                .map(|(service_id, service, service_action)| {
                    let queueing_record =
                        metrics_registry.start_record(service_id, StepLabel::Service, StepName::DeploymentQueueing);
                    let deployed_services = self.deployed_services.clone();
                    let opt_router = Self::get_associated_router(&target.environment.routers, service_id);
                    let deferred_routers = &deferred_routers;
                    move || {
                        queueing_record.stop(StepStatus::Success);

                        // creating services first
                        deployed_services.lock().unwrap().insert(service_id);
                        service.exec_action(target, service_action)?;

                        // then routers
                        if let Some(router) = opt_router.filter(|router| !deferred_routers.contains(router.long_id())) {
                            deployed_services.lock().unwrap().insert(*router.long_id());
                            return router.exec_action(target, *router.action());
                        }
                        Ok(())
                    }
                })
                .collect_vec(),
            router_tasks,
            || should_abort().is_err(),
            NonZeroUsize::new(parallel_deploys)
                .unwrap_or(NonZeroUsize::new(1).expect("error trying to instantiate NonZeroUsize")),
        )?;

        // clean up nlb
        if let Err(err) = clean_up_deleted_k8s_nlb(event_details.clone(), target) {
            error!(
                "clean_up_deleted_k8s_nlb fails: {}",
                err.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)
            )
        }

        Ok(())
    }

    pub fn on_pause(&mut self) -> Result<(), Box<EngineError>> {
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Pause);
        let target = Arc::new(&self.deployment_target);

        let should_abort = Self::should_abort_wrapper(&target, &event_details);
        should_abort()?;

        // reverse order of the deployment
        let services_to_pause = Self::services_without_routers_iter(target.environment).rev();
        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);

        self.logger
            .log(proceeding_event(&event_details, parallel_deploys, "pause(s)"));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        deployment_threads_pool.run(
            services_to_pause
                .into_iter()
                .map(|(service_id, service, _service_action)| {
                    let deployed_services = self.deployed_services.clone();
                    let local_target = target.clone();
                    let opt_router = Self::get_associated_router(&target.environment.routers, service_id);
                    move || {
                        // pausing routers
                        if let Some(router) = opt_router {
                            let _ = deployed_services.lock().map(|mut v| v.insert(*router.long_id()));
                            router.on_pause(&local_target)?;
                        }

                        // then services
                        let _ = deployed_services.lock().map(|mut v| v.insert(service_id));
                        service.on_pause(&local_target)
                    }
                })
                .collect_vec(),
            || should_abort().is_err(),
            NonZeroUsize::new(parallel_deploys)
                .unwrap_or(NonZeroUsize::new(1).expect("error trying to instantiate NonZeroUsize")),
        )?;

        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: event_details.clone(),
        };
        ns.on_pause(&target)?;

        Ok(())
    }

    pub fn on_delete(&mut self) -> Result<(), Box<EngineError>> {
        let target = &self.deployment_target;
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Delete);

        // re-create namespace first, because job can have on-delete action, so ns need to exist for us to run them
        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: event_details.clone(),
        };
        ns.on_create(target)?;

        let should_abort = Self::should_abort_wrapper(target, &event_details);
        should_abort()?;

        // reverse order of the deployment
        let services_to_delete = Self::services_without_routers_iter(target.environment).rev();

        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);

        self.logger
            .log(proceeding_event(&event_details, parallel_deploys, "delete(s)"));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        deployment_threads_pool.run(
            services_to_delete
                .into_iter()
                .map(|(service_id, service, _service_action)| {
                    let deployed_services = self.deployed_services.clone();
                    let opt_router = Self::get_associated_router(&target.environment.routers, service_id);
                    move || {
                        // deleting routers
                        if let Some(router) = opt_router {
                            let _ = deployed_services.lock().map(|mut v| v.insert(*router.long_id()));
                            router.on_delete(target)?;
                        }

                        // then services
                        let _ = deployed_services.lock().map(|mut v| v.insert(service_id));
                        service.on_delete(target)
                    }
                })
                .collect_vec(),
            || should_abort().is_err(),
            NonZeroUsize::new(parallel_deploys)
                .unwrap_or(NonZeroUsize::new(1).expect("error trying to instantiate NonZeroUsize")),
        )?;

        let ns = NamespaceDeployment {
            resource_expiration: target
                .kubernetes
                .context()
                .resource_expiration_in_seconds()
                .map(|ttl| Duration::from_secs(ttl as u64)),
            event_details: event_details.clone(),
        };
        ns.on_delete(target)?;

        Ok(())
    }

    pub fn on_restart(&mut self) -> Result<(), Box<EngineError>> {
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Restart);
        let target = Arc::new(&self.deployment_target);

        let should_abort = Self::should_abort_wrapper(&target, &event_details);
        should_abort()?;

        let services_to_restart = Self::services_without_routers_iter(target.environment);

        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);

        self.logger
            .log(proceeding_event(&event_details, parallel_deploys, "restart(s)"));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        deployment_threads_pool.run(
            services_to_restart
                .into_iter()
                .map(|(service_id, service, _service_action)| {
                    let deployed_services = self.deployed_services.clone();
                    let local_target = target.clone();
                    let opt_router = Self::get_associated_router(&target.environment.routers, service_id);
                    move || {
                        // restarting services
                        let _ = deployed_services.lock().map(|mut v| v.insert(service_id));
                        service.on_restart(&local_target)?;

                        // then router
                        if let Some(router) = opt_router {
                            let _ = deployed_services.lock().map(|mut v| v.insert(*router.long_id()));
                            return router.on_restart(&local_target);
                        }
                        Ok(())
                    }
                })
                .collect_vec(),
            || should_abort().is_err(),
            NonZeroUsize::new(parallel_deploys)
                .unwrap_or(NonZeroUsize::new(1).expect("error trying to instantiate NonZeroUsize")),
        )?;

        Ok(())
    }

    fn get_associated_router(routers: &'a [Box<dyn RouterService>], service_id: Uuid) -> Option<&'a dyn RouterService> {
        routers
            .iter()
            .find(|router| router.associated_service_id() == Some(service_id))
            .map(|router| router.as_ref())
    }
}

fn router_shares_hostname_listener_set(
    router_id: &Uuid,
    hostname: &str,
    mapped_routers: &BTreeSet<Uuid>,
    plans: &[(Uuid, &[CustomDomain], HostnameListenerSetPlan)],
) -> bool {
    mapped_routers.len() >= 2
        && mapped_routers.contains(router_id)
        && plans
            .iter()
            .any(|(id, _, plan)| mapped_routers.contains(id) && plan.certificate_hostnames.contains(hostname))
}

// A shared release may need namespaces created by a peer's Helm service. Defer only that hostname's
// routers, preserving inline service/router deployment when the namespaces already exist.
fn routers_waiting_for_namespaces<E>(
    plans: &[(Uuid, &[CustomDomain], HostnameListenerSetPlan)],
    mapping: &HostnameMapping,
    environment_namespace: &str,
    mut namespace_exists: impl FnMut(&str) -> Result<bool, E>,
) -> Result<HashSet<Uuid>, E> {
    let mut deferred = HashSet::new();
    let mut existing = BTreeMap::new();
    for (router_id, domains, plan) in plans {
        for (hostname, namespaces) in &plan.namespaces {
            let Some(routers) = mapped_hostname_routers(mapping, hostname, domains) else {
                continue;
            };
            if !router_shares_hostname_listener_set(router_id, hostname, &routers, plans) {
                continue;
            }
            for namespace in namespaces
                .iter()
                .filter(|namespace| namespace.as_str() != environment_namespace)
            {
                let exists = match existing.get(namespace) {
                    Some(exists) => *exists,
                    None => {
                        let exists = namespace_exists(namespace)?;
                        existing.insert(namespace, exists);
                        exists
                    }
                };
                if !exists {
                    deferred.extend(&routers);
                    break;
                }
            }
        }
    }
    Ok(deferred)
}

struct DeploymentThreadsPool {}

impl DeploymentThreadsPool {
    pub fn new() -> Self {
        Self {}
    }

    fn run_in_phases<Err, FirstTask, SecondTask>(
        &self,
        first_tasks: Vec<FirstTask>,
        second_tasks: Vec<SecondTask>,
        should_abort: impl Fn() -> bool + Send + Sync,
        max_parallelism: NonZeroUsize,
    ) -> Result<(), Err>
    where
        Err: Send + Clone,
        FirstTask: FnMut() -> Result<(), Err> + Send,
        SecondTask: FnMut() -> Result<(), Err> + Send,
    {
        self.run(first_tasks, &should_abort, max_parallelism)?;
        self.run(second_tasks, should_abort, max_parallelism)
    }

    pub fn run<Err, Task>(
        &self,
        tasks: Vec<Task>,
        should_abort: impl Fn() -> bool + Send + Sync,
        max_parallelism: NonZeroUsize,
    ) -> Result<(), Err>
    where
        Err: Send + Clone,
        Task: FnMut() -> Result<(), Err> + Send,
    {
        let max_parallelism = min(max_parallelism.get(), tasks.len());

        // Launch our thread-pool
        let current_thread = thread::current();
        thread::scope(|scope| {
            let mut ret: Result<(), Err> = Ok(());
            let mut active_threads: VecDeque<ScopedJoinHandle<Result<(), Err>>> =
                VecDeque::with_capacity(max_parallelism);

            let handle_thread_result = |th_result: thread::Result<Result<(), Err>>, ret: &mut Result<(), Err>| {
                match th_result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        // We want to store only the first error
                        if ret.is_ok() {
                            *ret = Err(err);
                        }
                    }
                    Err(err) => panic!("Deployment thread panicked: {err:?}"),
                }
            };

            let await_deployment_slot =
                |active_threads: &mut VecDeque<ScopedJoinHandle<_>>| -> thread::Result<Result<(), Err>> {
                    if active_threads.len() < max_parallelism {
                        return Ok(Ok(()));
                    }

                    // There is no available deployment slot, so we wait for a thread to terminate
                    let terminated_thread_ix = loop {
                        match active_threads.iter().position(|th| th.is_finished()) {
                            // timeout is needed because we call unpark within the thread
                            // So it can happens that we got unparked but the thread is not marked as finished yet
                            None => thread::park_timeout(Duration::from_secs(10)),
                            Some(position) => break position,
                        }
                    };

                    active_threads.swap_remove_back(terminated_thread_ix).unwrap().join()
                };

            // Launch our deployment in parallel for each service
            for (ix, mut task) in tasks.into_iter().enumerate() {
                // Ensure we have a slot available to run a new thread
                let thread_result = await_deployment_slot(&mut active_threads);
                handle_thread_result(thread_result, &mut ret);

                // If an abort arises, we just stop executing next tasks
                if should_abort() || ret.is_err() {
                    break;
                }

                // We have a slot to run a new thread, so start a new deployment
                let th = thread::Builder::new()
                    .name(format!("deployer-{ix}"))
                    .spawn_scoped(scope, {
                        let current_span = tracing::Span::current();
                        let current_thread = &current_thread;

                        move || {
                            let _span = current_span.enter();
                            let _guard = scopeguard::guard((), |_| current_thread.unpark());
                            task()
                        }
                    });
                active_threads.push_back(th.unwrap());
            }

            // Wait for all threads to terminate
            for th in active_threads {
                handle_thread_result(th.join(), &mut ret);
            }

            ret
        })
    }
}

/// Opens the services' deployment, so it is logged under that step rather than the one before it.
fn proceeding_event(event_details: &EventDetails, parallel_deploys: usize, actions: &str) -> EngineEvent {
    EngineEvent::Info(
        event_details
            .clone()
            .with_step_name_if_unset(Some(StepName::Deployment)),
        EventMessage::new_from_safe(format!("🎡 Proceeding with up to {parallel_deploys} parallel {actions}")),
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::BTreeSet;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    #[test]
    fn the_proceeding_line_opens_the_deployment_step() {
        let details = EventDetails::new(
            None,
            crate::io_models::QoveryIdentifier::new_random(),
            crate::io_models::QoveryIdentifier::new_random(),
            crate::io_models::QoveryIdentifier::new_random().to_string(),
            crate::events::Stage::Environment(EnvironmentStep::Deploy),
            crate::events::Transmitter::Environment(uuid::Uuid::new_v4(), "staging".to_string()),
        );

        let event = proceeding_event(&details, 7, "deployment(s)");

        assert_eq!(event.get_details().step_name(), Some(StepName::Deployment));
        assert_eq!(
            event.message(crate::events::EventMessageVerbosity::SafeOnly),
            "🎡 Proceeding with up to 7 parallel deployment(s)"
        );
    }

    #[test]
    fn only_shared_routers_with_missing_peer_namespaces_are_deferred() {
        let router_a = uuid::uuid!("44444444-4444-4444-4444-444444444444");
        let router_b = uuid::uuid!("55555555-5555-5555-5555-555555555555");
        let outsider = uuid::uuid!("66666666-6666-6666-6666-666666666666");
        let mapping: HostnameMapping = serde_json::from_value(serde_json::json!({
            "api.example.com": [router_a, router_b], "other.example.com": [outsider]
        }))
        .unwrap();
        let domains = [CustomDomain {
            domain: "api.example.com".to_string(),
            target_domain: String::new(),
            generate_certificate: true,
            use_cdn: false,
        }];
        let plan = |hostname: &str, namespace: &str| HostnameListenerSetPlan {
            namespaces: BTreeMap::from([(hostname.to_string(), BTreeSet::from([namespace.to_string()]))]),
            certificate_hostnames: BTreeSet::from([hostname.to_string()]),
        };
        let mut plans = vec![
            (router_a, domains.as_slice(), plan("api.example.com", "env-ns")),
            (router_b, domains.as_slice(), plan("api.example.com", "env-ns")),
            (outsider, domains.as_slice(), plan("other.example.com", "unrelated-ns")),
        ];
        let deferred = routers_waiting_for_namespaces::<()>(&plans, &mapping, "env-ns", |_| {
            panic!("environment namespace exists")
        });
        assert!(
            deferred.unwrap().is_empty(),
            "ordinary service/router ordering must be preserved"
        );
        plans[1].2 = plan("api.example.com", "peer-ns");
        for exists in [true, false] {
            let deferred = routers_waiting_for_namespaces(&plans, &mapping, "env-ns", |namespace| {
                assert_eq!(namespace, "peer-ns", "unrelated namespaces must not delay inline routers");
                Ok::<_, ()>(exists)
            })
            .unwrap();
            assert_eq!(
                deferred,
                if exists {
                    HashSet::new()
                } else {
                    HashSet::from([router_a, router_b])
                }
            );
        }
    }

    #[test]
    fn shared_hostname_routers_wait_for_all_services_even_with_one_deployment_worker() {
        for parallelism in [1, 3] {
            let services_ready = AtomicUsize::new(0);
            let router_count = AtomicUsize::new(0);
            let services = (0..3)
                .map(|_| {
                    || {
                        services_ready.fetch_add(1, Ordering::SeqCst);
                        Ok::<_, ()>(())
                    }
                })
                .collect();
            let routers = (0..3)
                .map(|_| {
                    || {
                        assert_eq!(
                            services_ready.load(Ordering::SeqCst),
                            3,
                            "peer namespaces must exist before ListenerSet apply"
                        );
                        router_count.fetch_add(1, Ordering::SeqCst);
                        Ok::<_, ()>(())
                    }
                })
                .collect();
            DeploymentThreadsPool::new()
                .run_in_phases(services, routers, || false, NonZeroUsize::new(parallelism).unwrap())
                .unwrap();
            assert_eq!(router_count.load(Ordering::SeqCst), 3);
        }
    }

    #[test]
    fn failed_service_prevents_shared_hostname_router_deployments() {
        let services = vec![|| Err("service failed")];
        let routers = vec![|| -> Result<(), &str> { panic!("routers must not run after a service failure") }];
        assert_eq!(
            DeploymentThreadsPool::new().run_in_phases(services, routers, || false, NonZeroUsize::new(1).unwrap()),
            Err("service failed")
        );
    }

    #[test]
    fn test_deployment_thread_pool_parallelism() {
        // setup:
        const TASKS_COUNT: usize = 10;
        type MaxParallelDeploy = usize;
        let test_cases: Vec<MaxParallelDeploy> = vec![1, 3, 5];

        let pool = DeploymentThreadsPool::new();

        for tc in test_cases {
            // execute:
            let active_tasks = AtomicUsize::new(0);
            let mut tasks = Vec::new();
            for _ in 0..TASKS_COUNT {
                tasks.push(|| {
                    active_tasks.fetch_add(1, Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(100));
                    Result::<(), ()>::Ok(())
                });
            }

            let result = pool.run(tasks, || false, NonZeroUsize::new(tc).unwrap());

            // verify:
            assert!(result.is_ok());
            assert_eq!(active_tasks.load(Ordering::Relaxed), TASKS_COUNT);
        }
    }

    #[test]
    fn test_deployment_thread_pool_max_parallelism() {
        // setup:
        const TASKS_COUNT: usize = 10;
        type MaxParallelDeploy = usize;
        let test_cases: Vec<MaxParallelDeploy> = vec![1, 3, 5];

        let pool = DeploymentThreadsPool::new();

        for tc in test_cases {
            // execute:
            let active_tasks = AtomicUsize::new(0);
            let max_active_task = AtomicUsize::new(0);
            let mut tasks = Vec::new();
            for _ in 0..TASKS_COUNT {
                tasks.push(|| {
                    let nb_tasks = active_tasks.fetch_add(1, Ordering::Relaxed);
                    max_active_task.fetch_max(nb_tasks + 1, Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(1000));
                    active_tasks.fetch_sub(1, Ordering::Relaxed);
                    Result::<(), ()>::Ok(())
                });
            }

            let result = pool.run(tasks, || false, NonZeroUsize::new(tc).unwrap());

            // verify:
            assert!(result.is_ok());
            assert_eq!(active_tasks.load(Ordering::Relaxed), 0);
            assert_eq!(max_active_task.load(Ordering::Relaxed), tc);
        }
    }

    #[test]
    fn test_deployment_thread_pool_error_cancelling_other_tasks() {
        // setup:
        const TASKS_COUNT: usize = 10;
        const FAILING_TASK_NUMBER: usize = 1;
        const MAX_PARALLEL_DEPLOYS: usize = 2;

        let pool = DeploymentThreadsPool::new();

        // execute:
        let active_tasks = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for i in 0..TASKS_COUNT {
            let active_tasks_local = active_tasks.clone();
            tasks.push(move || {
                active_tasks_local.fetch_add(1, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(1000));
                match i == FAILING_TASK_NUMBER {
                    true => Result::<(), ()>::Err(()),
                    false => Result::<(), ()>::Ok(()),
                }
            });
        }

        let ret = pool.run(tasks, || false, NonZeroUsize::new(MAX_PARALLEL_DEPLOYS).unwrap());

        // verify:
        assert!(ret.is_err());

        // Avoiding flakiness, we test that not all tasks are being executed
        assert!(active_tasks.load(Ordering::Relaxed) < TASKS_COUNT);
    }
}
