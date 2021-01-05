#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate prometheus;

pub mod models;
pub mod task_manager;
pub mod tasks;

pub fn log_debug_no_spam_builder(msg: &str, every_n_times: u32) -> Box<dyn FnMut()> {
    if log_enabled!(log::Level::Debug) {
        let mut loop_counter= 0;
        let msg = msg.to_string();
        Box::new(move || {
            if loop_counter % every_n_times == 0 {
                debug!("{}", msg);
                loop_counter = 1;
            }
            loop_counter +=1;
        })
    } else {
        Box::new(#[inline(always)] || {})
    }
}
