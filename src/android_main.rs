use crate::{
    app::build::PolarBearApp,
    utils::{
        application_context::ApplicationContext,
        config,
        fullscreen_immersive::{enable_fullscreen_immersive_mode, keep_screen_on},
        logging::PolarBearExpectation,
        ndk::run_in_jvm,
    },
};
use sentry::integrations::log::{LogFilter, SentryLogger};
use winit::{
    event_loop::{ControlFlow, EventLoop},
    platform::android::{activity::AndroidApp, EventLoopBuilderExtAndroid},
};

#[no_mangle]
fn android_main(android_app: AndroidApp) {
    std::env::set_var("RUST_BACKTRACE", "full");
    let _guard = sentry::init((
        config::SENTRY_DSN,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            // Capture user IPs and potentially sensitive headers when using HTTP server integrations
            // see https://docs.sentry.io/platforms/rust/data-management/data-collected for more info
            send_default_pii: true,
            enable_logs: true,
            ..Default::default()
        },
    ));

    // Wrap the Android logger with Sentry's logger
    let logger = SentryLogger::with_dest(android_logger::AndroidLogger::default()).filter(|md| {
        match md.level() {
            // Capture error records as Sentry events
            // These are grouped into issues, representing high-severity errors to act upon
            log::Level::Error => LogFilter::Event,
            // Ignore trace level records, as they're too verbose
            log::Level::Trace => LogFilter::Ignore,
            // Capture everything else as a log
            _ => LogFilter::Log,
        }
    });
    log::set_boxed_logger(Box::new(logger)).pb_expect("Failed to set Sentry logger");
    #[cfg(debug_assertions)] // Enable verbose logging in debug builds
    log::set_max_level(log::LevelFilter::Trace);
    #[cfg(not(debug_assertions))]
    log::set_max_level(log::LevelFilter::Info);

    ApplicationContext::build(&android_app);

    run_in_jvm(enable_fullscreen_immersive_mode, android_app.clone());
    run_in_jvm(keep_screen_on, android_app.clone());

    let event_loop = EventLoop::builder()
        .with_android_app(android_app.clone())
        .build()
        .pb_expect("Failed to create event loop");

    // ControlFlow::Poll continuously runs the event loop, even if the OS hasn't
    // dispatched any events. This is ideal for games and similar applications.
    // event_loop.set_control_flow(ControlFlow::Poll);

    // ControlFlow::Wait pauses the event loop if no events are available to process.
    // This is ideal for non-game applications that only update in response to user
    // input, and uses significantly less power/CPU time than ControlFlow::Poll.
    event_loop.set_control_flow(ControlFlow::Wait);

    // Phase 1: Setup
    let mut app = PolarBearApp::build(android_app);

    // Phase 2: Run
    event_loop.run_app(&mut app).pb_expect("Failed to run app");
}
