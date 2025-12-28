use std::{
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use common::security;
use tracing::{info, warn};

use crate::chat::{
    message,
    room::{Error as RoomError, MessageQueue, RecvError, get_room},
    user::{UserRegistry, get_registry},
};

const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_millis(100);
const DEFAULT_RECV_TIMEOUT: Duration = Duration::from_millis(100);
static BROKER: LazyLock<MessageBroker> = LazyLock::new(|| {
    let broker = MessageBroker::new();
    broker.start_dispatcher();
    broker
});

pub fn get_broker() -> &'static MessageBroker {
    &BROKER
}

pub struct MessageBroker {
    room: &'static dyn MessageQueue,
    registry: &'static UserRegistry,
    dispatcher_handle: Mutex<Option<JoinHandle<()>>>,
    shutdown_flag: Arc<AtomicBool>,
}

impl MessageBroker {
    fn new() -> Self {
        Self {
            room: get_room(),
            registry: get_registry(),
            dispatcher_handle: Mutex::new(None),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub const fn registry(&self) -> &UserRegistry {
        self.registry
    }
    // our use case is broadcase to all
    pub fn forward_to_room(&self, msg: String) -> Result<(), RoomError> {
        self.room.send_timeout(msg, DEFAULT_SEND_TIMEOUT)
    }
    fn start_dispatcher(&self) {
        let receiver = self.room.receiver();
        let registry = self.registry;
        let shutdown_flag = Arc::clone(&self.shutdown_flag);

        let handle = thread::spawn(move || {
            loop {
                if shutdown_flag.load(Ordering::Relaxed) {
                    info!("Dispatcher received shutdown signal");
                    break;
                }

                match receiver.recv_timeout(DEFAULT_RECV_TIMEOUT) {
                    Ok(serialized) => {
                        if let Some(msg) = message::ChatMessage::deserialize(&serialized) {
                            let sent = registry.broadcast(&msg.1, Some(&msg.0)).unwrap_or(0);
                            if sent > 0 {
                                let safe_username = security::sanitize_for_log(&msg.0.to_string());
                                info!("Dispatched message from '{}' to {} users", safe_username, sent);
                            }
                        } else {
                            let safe_msg = security::truncate_for_log(&security::sanitize_for_log(&serialized), 100);
                            warn!("Failed to deserialize message: {safe_msg}");
                        }
                    }
                    Err(RecvError::Timeout) => {}
                    Err(RecvError::Disconnected) => {
                        info!("Room channel disconnected, stopping dispatcher");
                        break;
                    }
                }
            }

            info!("Dispatcher thread stopped");
        });

        if let Ok(mut guard) = self.dispatcher_handle.lock() {
            *guard = Some(handle);
        }
    }
}
