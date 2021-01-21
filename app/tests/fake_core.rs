// use std::io::Error;
// use std::str::from_utf8;
// use std::thread::sleep;
// use std::time::Duration;

/*#[test]
fn fake_core_task_status_receiver() -> Result<(), Error> {
    let nc = nats::Options::new()
        .with_name("fake_core")
        //.tls_connector(tls_connector) // FIXME
        .connect("localhost:4222")?;

    let sub = nc.subscribe("core.task.status")?;

    sub.with_handler(|msg| {
        let json = from_utf8(msg.data.as_slice());
        println!("{}", json.unwrap());
        let _ = msg.respond("ok");
        Ok(())
    });

    let _ = sleep(Duration::from_secs(10_000));

    Ok(())
}
*/
