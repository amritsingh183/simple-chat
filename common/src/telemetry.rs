use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config;
/// Initialize the logging/tracing subsystem.
///
/// Returns a `WorkerGuard` that must be kept alive for the duration of the program
/// to ensure all log messages are flushed.
///
/// The logging format depends on the `APP_ENV` environment variable:
/// - `production`: JSON format
/// - other: Pretty format (default)
///
/// # Errors
///
/// Returns an error if the tracing subscriber fails to initialize
/// (e.g., if it has already been initialized).
pub fn init_logging() -> Result<WorkerGuard, Box<dyn std::error::Error + Send + Sync>> {
    let _ = config::get_server_tz()?;
    let log_level = config::log_level()?;
    let app_env = config::app_env();
    let env_filter = EnvFilter::try_new(&log_level)?;
    let (non_blocking_writer, guard) = tracing_appender::non_blocking(std::io::stdout());
    let formatting_layer = if app_env == config::APP_ENV_PROD_VALUE {
        fmt::layer()
            .json()
            .flatten_event(true)
            .with_writer(non_blocking_writer)
            .boxed()
    } else {
        fmt::layer().pretty().with_writer(non_blocking_writer).boxed()
    };
    Registry::default().with(env_filter).with(formatting_layer).try_init()?;

    Ok(guard)
}
