mod task_manager;
mod tasks;

use std::io::Error;
use std::thread::sleep;
use std::time::Duration;

fn main() -> Result<(), Error> {
    let nc = nats::Options::new()
        .with_name("qovery-engine-app")
        .connect("localhost:4222")?;

    let sub = nc.queue_subscribe("my.subject", "qovery-engine-app")?;

    sub.with_handler(move |msg| {
        println!("{}", msg);
        msg.respond("done");
        Ok(())
    });

    loop {
        nc.request("my.subject", "hey hey hey");
        sleep(Duration::from_secs(1));
    }

    Ok(())
}
