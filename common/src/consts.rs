use std::time::Duration;

pub const BACKBONE_DEFAULT_SEND_TIMEOUT: Duration = Duration::from_millis(100);
pub const BACKBONE_DEFAULT_RECV_TIMEOUT: Duration = Duration::from_millis(100);
pub const ENV_CHAT_HOST: &str = "CHAT_HOST";
pub const ENV_CHAT_PORT: &str = "CHAT_PORT";
pub const ENV_CHAT_USERNAME: &str = "CHAT_USERNAME";

pub const SERVER_EVENT_OK: &str = "OK";
pub const SERVER_EVENT_OK_PREFIX: &str = "OK";

pub const SERVER_EVENT_BROADCAST: &str = "BROADCAST";
pub const SERVER_EVENT_BROADCAST_PREFIX: &str = "BROADCAST ";

pub const SERVER_EVENT_ERR: &str = "ERR";
pub const SERVER_EVENT_ERR_PREFIX: &str = "ERR ";

pub const SERVER_EVENT_USER_JOINED: &str = "JOINED";
pub const SERVER_EVENT_USER_JOINED_PREFIX: &str = "JOINED ";

pub const SERVER_EVENT_USER_LEFT: &str = "LEFT";
pub const SERVER_EVENT_USER_LEFT_PREFIX: &str = "LEFT ";

pub const CLIENT_JOIN_CMD: &str = "JOIN";
pub const CLIENT_JOIN_PREFIX: &str = "JOIN";

pub const CLIENT_SEND_CMD: &str = "SEND";
pub const CLIENT_SEND_PREFIX: &str = "SEND ";

pub const CLIENT_LEAVE_CMD: &str = "LEAVE";
pub const CLIENT_LEAVE_PREFIX: &str = "LEAVE ";

pub const APP_ENV: &str = "CHAT_APP_ENV";
pub const DEFAULT_LOG_LEVEL: &str = "CHAT_APP_LOG_LEVEL";
pub const DEFAULT_TZ: &str = "Etc/UTC";
pub const APP_ENV_DEFAULT_VALUE: &str = "development";
pub const APP_ENV_PROD_VALUE: &str = "production";

pub const DEFAULT_LOG_LEVEL_DEFAULT_VALUE: &str = "info";

pub const MAX_LOG_LINE_LENGTH: usize = 1024;
pub const CHECK_INTERVAL_TCP_READER: Duration = Duration::from_millis(600);

pub const MAX_CLIENT_BUFFER_SIZE: usize = 10;

pub const MAX_CLIENT_MESSAGE_LENGTH: usize = 4096;

/// Read timeout for socket operations.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum concurrent connections the server will accept.
pub const MAX_CONNECTIONS: usize = 10_000;

/// Rate limit: maximum messages per second per user.
pub const MAX_MESSAGES_PER_SECOND: u32 = 10;

/// Rate limit: burst capacity for message rate limiting.
pub const MESSAGE_BURST_CAPACITY: u32 = 20;
