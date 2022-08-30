use maitake::{scheduler::Scheduler, sync::WaitQueue};
use std::{sync::Arc, thread, time::Duration};

fn main() {
    console_subscriber::init();
    eprintln!(
        "\ntokio-console listening on {:?}:{}\n",
        console_subscriber::Server::DEFAULT_IP,
        console_subscriber::Server::DEFAULT_PORT
    );

    let scheduler = Scheduler::new();
    loop {
        let q = Arc::new(WaitQueue::new());
        let mut completed = 0;
        for _ in 0..10 {
            let q = q.clone();
            scheduler.spawn(async move {
                let _ = q.wait().await;
                thread::sleep(Duration::from_secs(2));
                q.wake();
            });
        }
        scheduler.tick();
        q.wake();
        while completed < 10 {
            let tick = scheduler.tick();
            tracing_01::info!(?tick);
            completed += tick.completed;
        }
        thread::sleep(Duration::from_secs(1));
    }
}
