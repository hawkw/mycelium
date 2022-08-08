pub fn trace_init() {
    use tracing_subscriber::filter::LevelFilter;
    let _ = tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .with_test_writer()
        .try_init();
}
