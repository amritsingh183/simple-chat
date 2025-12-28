use std::{env, fmt};

pub const APP_ENV: &str = "CHAT_APP_ENV";
pub const DEFAULT_TZ: &str = "Etc/UTC";
pub const APP_ENV_DEFAULT_VALUE: &str = "development";
pub const APP_ENV_PROD_VALUE: &str = "production";
pub const DEFAULT_LOG_LEVEL: &str = "CHAT_APP_LOG_LEVEL";
pub const DEFAULT_LOG_LEVEL_DEFAULT_VALUE: &str = "info";

/// Returns the server timezone from the `TZ` environment variable.
///
/// # Errors
///
/// Returns an error if the `TZ` environment variable is not set.
#[must_use = "Server must use same timezone everywhere"]
pub fn get_server_tz() -> Result<String, String> {
    env::var("TZ").map_err(|_| "TZ must be set".to_string())
}

/// Error returned when an invalid log level is specified.
#[derive(Debug, Clone)]
pub struct InvalidLogLevelError {
    pub level: String,
}

impl fmt::Display for InvalidLogLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid log level: '{}'. Must be one of: trace, debug, info, warn, error, off",
            self.level
        )
    }
}

impl std::error::Error for InvalidLogLevelError {}

#[must_use]
pub fn app_env() -> String {
    env::var(APP_ENV).unwrap_or_else(|_| APP_ENV_DEFAULT_VALUE.to_owned())
}

/// Returns the configured log level.
///
/// # Errors
///
/// Returns an error if the log level is not one of: trace, debug, info, warn, error, off.
pub fn log_level() -> Result<String, InvalidLogLevelError> {
    let level = env::var(DEFAULT_LOG_LEVEL)
        .or_else(|_| env::var("RUST_LOG"))
        .unwrap_or_else(|_| DEFAULT_LOG_LEVEL_DEFAULT_VALUE.to_owned());

    // Validate it's a known log level
    match level.to_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" | "off" => Ok(level),
        _ => Err(InvalidLogLevelError { level }),
    }
}

#[must_use]
pub fn is_production() -> bool {
    app_env() == APP_ENV_PROD_VALUE
}
