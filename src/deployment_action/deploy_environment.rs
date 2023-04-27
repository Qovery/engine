use crate::cloud_provider::aws::load_balancers::clean_up_deleted_k8s_nlb;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::deploy_namespace::NamespaceDeployment;
use crate::deployment_action::DeploymentAction;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessage};
use crate::logger::Logger;
use crate::runtime::block_on;
use itertools::Itertools;
use k8s_openapi::api::core::v1::Namespace;
use kube::api::ListParams;
use kube::Api;
use std::cmp::{max, min};
use std::collections::{HashSet, VecDeque};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::thread;
use std::thread::ScopedJoinHandle;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct EnvironmentDeployment<'a> {
    pub deployed_services: Arc<Mutex<HashSet<Uuid>>>,
    deployment_target: DeploymentTarget<'a>,
    logger: Arc<Box<dyn Logger>>,
}

impl<'a> EnvironmentDeployment<'a> {
    pub fn new(
        infra_ctx: &'a InfrastructureContext,
        environment: &'a Environment,
        should_abort: &'a (dyn Fn() -> bool + Send + Sync),
        logger: Arc<Box<dyn Logger>>,
    ) -> Result<EnvironmentDeployment<'a>, Box<EngineError>> {
        let deployment_target = DeploymentTarget::new(infra_ctx, environment, should_abort)?;
        Ok(EnvironmentDeployment {
            deployed_services: Arc::new(Mutex::new(HashSet::with_capacity(Self::services_iter(environment).count()))),
            deployment_target,
            logger,
        })
    }

    fn services_iter(
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
                    .routers
                    .iter()
                    .map(|s| (*s.long_id(), s.as_deployment_action(), *s.action())),
            )
    }

    fn should_abort_wrapper<'b>(
        target: &'b DeploymentTarget,
        event_details: &'b EventDetails,
    ) -> impl Fn() -> Result<(), Box<EngineError>> + 'b + Send + Sync {
        move || {
            if (target.should_abort)() {
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

        let should_abort = Self::should_abort_wrapper(target, &event_details);
        should_abort()?;

        // deploy namespace first
        let ns = NamespaceDeployment {
            resource_expiration,
            event_details: event_details.clone(),
        };
        ns.exec_action(target, target.environment.action)?;

        let services_to_deploy = Self::services_iter(target.environment);
        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);

        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Proceeding with up to {} parallel deployment(s)", parallel_deploys)),
        ));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        deployment_threads_pool.run(
            services_to_deploy
                .into_iter()
                .map(|(service_id, service, service_action)| {
                    let deployed_services = self.deployed_services.clone();
                    move || {
                        deployed_services.blocking_lock().insert(service_id);
                        service.exec_action(target, service_action)
                    }
                })
                .collect_vec(),
            || should_abort().is_err(),
            NonZeroUsize::new(parallel_deploys)
                .unwrap_or(NonZeroUsize::new(1).expect("error trying to instantiate NonZeroUsize")),
        )?;

        // clean up nlb
        clean_up_deleted_k8s_nlb(event_details.clone(), target)?;

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
        let services_to_pause = Self::services_iter(target.environment).rev();
        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);

        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Proceeding with up to {} parallel pause(s)", parallel_deploys)),
        ));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        deployment_threads_pool.run(
            services_to_pause
                .into_iter()
                .map(|(service_id, service, _service_action)| {
                    let deployed_services = self.deployed_services.clone();
                    let local_target = target.clone();
                    move || {
                        deployed_services.blocking_lock().insert(service_id);
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
        let environment = &target.environment;
        let event_details = self
            .deployment_target
            .environment
            .event_details_with_step(EnvironmentStep::Delete);

        // check if environment is not already deleted
        // speed up delete env because of terraform requiring apply + destroy
        let api: Api<Namespace> = Api::all(target.kube.clone());
        let envs = block_on(api.list(&ListParams::default())).map_err(|e| {
            EngineError::new_k8s_describe(
                event_details.clone(),
                "list namespace".to_string(),
                environment.namespace().to_string(),
                CommandError::from(e),
            )
        })?;

        if !envs
            .items
            .iter()
            .any(|ns| ns.metadata.name.as_deref().unwrap_or("") == environment.namespace())
        {
            info!("no need to delete environment {}, already absent", environment.namespace());
            Self::services_iter(target.environment).for_each(|(id, _, _)| {
                self.deployed_services.blocking_lock().insert(id);
            });
            return Ok(());
        }

        // reverse order of the deployment
        let should_abort = Self::should_abort_wrapper(target, &event_details);
        should_abort()?;

        let services_to_delete = Self::services_iter(target.environment).rev();

        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);

        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Proceeding with up to {} parallel delete(s)", parallel_deploys)),
        ));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        deployment_threads_pool.run(
            services_to_delete
                .into_iter()
                .map(|(service_id, service, _service_action)| {
                    let deployed_services = self.deployed_services.clone();
                    move || {
                        deployed_services.blocking_lock().insert(service_id);
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
            .event_details_with_step(EnvironmentStep::Pause);
        let target = Arc::new(&self.deployment_target);

        let should_abort = Self::should_abort_wrapper(&target, &event_details);
        should_abort()?;

        let services_to_restart = Self::services_iter(target.environment);

        let parallel_deploys = max(target.environment.max_parallel_deploy as usize, 1);

        self.logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Proceeding with up to {} parallel restart(s)", parallel_deploys)),
        ));

        let deployment_threads_pool = DeploymentThreadsPool::new();
        deployment_threads_pool.run(
            services_to_restart
                .into_iter()
                .map(|(service_id, service, _service_action)| {
                    let deployed_services = self.deployed_services.clone();
                    let local_target = target.clone();
                    move || {
                        deployed_services.blocking_lock().insert(service_id);
                        service.on_restart(&local_target)
                    }
                })
                .collect_vec(),
            || should_abort().is_err(),
            NonZeroUsize::new(parallel_deploys)
                .unwrap_or(NonZeroUsize::new(1).expect("error trying to instantiate NonZeroUsize")),
        )?;

        Ok(())
    }
}

struct DeploymentThreadsPool {}

impl DeploymentThreadsPool {
    pub fn new() -> Self {
        Self {}
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
            for mut task in tasks {
                // Ensure we have a slot available to run a new thread
                let thread_result = await_deployment_slot(&mut active_threads);
                handle_thread_result(thread_result, &mut ret);

                // If an abort arises, we just stop executing next tasks
                if should_abort() || ret.is_err() {
                    break;
                }

                // We have a slot to run a new thread, so start a new deployment
                let th = thread::Builder::new()
                    .name("deployer".to_string())
                    .spawn_scoped(scope, {
                        let current_thread = &current_thread;
                        move || {
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

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

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
        const FAILING_TASK_NUMBER: usize = 2;
        const MAX_PARALLEL_DEPLOYS: usize = 2;

        let pool = DeploymentThreadsPool::new();

        // execute:
        let active_tasks = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for i in 0..TASKS_COUNT {
            let active_tasks_local = active_tasks.clone();
            tasks.push(move || {
                active_tasks_local.fetch_add(1, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(100));
                match i == FAILING_TASK_NUMBER {
                    true => Result::<(), ()>::Err(()),
                    false => Result::<(), ()>::Ok(()),
                }
            });
        }

        let ret = pool.run(tasks, || false, NonZeroUsize::new(MAX_PARALLEL_DEPLOYS).unwrap());

        // verify:
        assert!(ret.is_err());
        assert_eq!(active_tasks.load(Ordering::Relaxed), 4);
    }
}
