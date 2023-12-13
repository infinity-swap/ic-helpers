use std::sync::Arc;

use ic_stable_structures::UnboundedMapStructure;
use parking_lot::Mutex;
use crate::SchedulerError;
use crate::task::{Task, ScheduledTask};
use crate::time::time_secs;

/// A scheduler is responsible for executing tasks.
pub struct Scheduler<T: 'static + Task, P: 'static + UnboundedMapStructure<u32, ScheduledTask<T>>> {
    pending_tasks: Arc<Mutex<P>>,
    phantom: std::marker::PhantomData<T>,
}

impl <T: 'static + Task, P: 'static + UnboundedMapStructure<u32, ScheduledTask<T>>> Scheduler<T, P> {

    pub fn new(pending_tasks: P) -> Self {
        Self {
            pending_tasks: Arc::new(Mutex::new(pending_tasks)),
            phantom: std::marker::PhantomData,
        }
    }

    /// Execute all pending tasks.
    /// Each task is executed asynchronously in a dedicated ic_cdk::spaw call. 
    /// Consequently, the scheduler does not wait for the tasks to finish.
    pub fn run(&self) -> Result<(), SchedulerError> {
        
        let mut to_be_reprocessed = Vec::new();
        {
            let mut lock = self.pending_tasks.lock();
            while let Some(key) = lock.first_key() {
                let task = lock.remove(&key);
                drop(lock);
                if let Some(task) = task {
                    let now_timestamp_secs = time_secs();

                    if task.options.execute_after_timestamp_in_secs > now_timestamp_secs {
                        to_be_reprocessed.push(task);
                    } else {
                        let task_scheduler = self.clone();
                        ic_kit::ic::spawn(async move {
                            match task.task.execute(Box::new(task_scheduler.clone())).await {
                                Ok(()) => {
//                                    processed_tasks.push(task);
                                },
                                Err(_) => {
                                    if task.options.max_retries > 0 {
                                        let mut task = task;
                                        task.options.max_retries = task.options.max_retries - 1;
                                        task.options.execute_after_timestamp_in_secs = now_timestamp_secs + task.options.retry_delay_secs;
                                        task_scheduler.append_task(task)
                                    }
                                },
                            }
                        });
                    }
                }
                lock = self.pending_tasks.lock();
            }
        }
        self.append_tasks(to_be_reprocessed);
        Ok(())
    }
}

pub trait TaskScheduler<T: 'static + Task> {
    fn append_task(&self, task: ScheduledTask<T>);
    fn append_tasks(&self, tasks: Vec<ScheduledTask<T>>);
}

impl <T: 'static + Task, P: 'static + UnboundedMapStructure<u32, ScheduledTask<T>>> Clone for Scheduler<T, P> {
    fn clone(&self) -> Self {
        Self { pending_tasks: self.pending_tasks.clone(), phantom: self.phantom.clone() }
    }
}

impl <T: 'static + Task, P: 'static + UnboundedMapStructure<u32, ScheduledTask<T>>> TaskScheduler<T> for Scheduler<T, P> {
    fn append_task(&self, task: ScheduledTask<T>) {
        let mut lock = self.pending_tasks.lock();
        let key = lock.last_key().map(|val| val + 1).unwrap_or_default();
        lock.insert(&key, &task);
    }

    fn append_tasks(&self, tasks: Vec<ScheduledTask<T>>) {
        if tasks.is_empty() {
            return;
        };

        let mut lock = self.pending_tasks.lock();
        let mut key = lock.last_key().map(|val| val + 1).unwrap_or_default();

        for task in tasks {
            lock.insert(&key, &task);
            key = key + 1;
        }
    }
}

#[cfg(test)] 
mod test {

    use std::{pin::Pin, future::Future, collections::HashMap, thread::sleep, time::Duration};

    use ic_kit::MockContext;
    use ic_stable_structures::{StableUnboundedMap, VectorMemory};
    use rand::random;
    use serde::{Deserialize, Serialize};
    
    use crate::task::TaskOptions;

    use super::*;

    thread_local! {
        pub static STATE: Mutex<HashMap<u32, Vec<String>>> = Mutex::new(HashMap::new())
    }

#[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum TestTask {
        StepOne{ id: u32 },
        StepTwo{ id: u32 },
        StepThree{ id: u32 },
    }

    impl Task for TestTask {
        fn execute(&self, task_scheduler: Box<dyn 'static + TaskScheduler<Self>>) -> Pin<Box<dyn Future<Output = Result<(), SchedulerError>>>> {
            match self {
                TestTask::StepOne{ id } => {
                    let id = *id;
                Box::pin(async move {
                    let msg = format!("{} - StepOne", id);
                    println!("{}", msg);
                    STATE.with(|state| {
                        let mut state = state.lock();
                        let entry = state.entry(id).or_insert_with(Vec::new);
                        entry.push(msg);
                    });
                    std::thread::sleep(Duration::from_millis(500));
                    // Append the next task to be executed
                    task_scheduler.append_task(TestTask::StepTwo{ id }.into());
                    Ok(())
                })},
                TestTask::StepTwo{ id } => {
                    let id = *id;
                Box::pin(async move {
                    let msg = format!("{} - StepTwo", id);
                    println!("{}", msg);
                    STATE.with(|state| {
                        let mut state = state.lock();
                        let entry = state.entry(id).or_insert_with(Vec::new);
                        entry.push(msg);
                    });
                    // More tasks can be appended to the scheduler. BEWARE of circular dependencies!!
                    task_scheduler.append_task(TestTask::StepThree{ id }.into());
                    task_scheduler.append_task(TestTask::StepThree{ id }.into());
                    Ok(())
                })},
                TestTask::StepThree{ id } => {
                    let id = *id;
                Box::pin(async move {
                    let msg = format!("{} - Done", id);
                    println!("{}", msg);
                    STATE.with(|state| {
                        let mut state = state.lock();
                        let entry = state.entry(id).or_insert_with(Vec::new);
                        entry.push(msg);
                    });
                    // the last task does not append anything to the scheduler
                    Ok(())
                })},
            }
        }
    }

    #[tokio::test]
    async fn test_run_scheduler() {
        MockContext::new().inject();
        let map = StableUnboundedMap::new(VectorMemory::default());
        let scheduler = Scheduler::new(map);
        let id = random();
        scheduler.append_task(TestTask::StepOne{id}.into());
        scheduler.run().unwrap();

        // MockContext executes and waits the spawned tasks in the same thread
        // so the scheduler job is always completed.
        STATE.with(|state| {
            let state = state.lock();
            let messages = state.get(&id).cloned().unwrap_or_default();
                assert_eq!(messages, vec![
                    format!("{} - StepOne", id),
                    format!("{} - StepTwo", id),
                    format!("{} - Done", id),
                    format!("{} - Done", id),
                ]);
        });

    }

    #[tokio::test]
    async fn test_task_option_execute_after_timestamp() {
        let map = StableUnboundedMap::new(VectorMemory::default());
        let scheduler = Scheduler::new(map);
        let id = random();
        scheduler.append_task((TestTask::StepOne{id}, TaskOptions::new()
            .with_execute_after_timestamp_in_secs(time_secs() + 2)
        ).into());

        scheduler.run().unwrap();

        todo!()
    }

    #[tokio::test]
    async fn test_task_failure_and_retry() {
        let map = StableUnboundedMap::new(VectorMemory::default());
        let scheduler = Scheduler::new(map);
        let id = random();
        scheduler.append_task((TestTask::StepOne{id}, TaskOptions::new()
            .with_max_retries(3)
        ).into());
        
        scheduler.run().unwrap();

        todo!()
    }

    #[tokio::test]
    async fn test_task_retry_delay() {
        let map = StableUnboundedMap::new(VectorMemory::default());
        let scheduler = Scheduler::new(map);
        let id = random();
        scheduler.append_task((TestTask::StepOne{id}, TaskOptions::new()
            .with_max_retries(5)
            .with_retry_delay_secs(2)
        ).into());
        
        scheduler.run().unwrap();

        todo!()
    }

}