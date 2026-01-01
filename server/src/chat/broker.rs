use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};

use common::consts;
use tokio::{sync::Mutex, task::JoinHandle};
use tracing::info;

use crate::chat::{
    room::{Error as RoomError, MessageQueue, MessageReceiver, OneToMany, OneToOne, RecvError, get_room},
    user::{UserRegistry, get_registry},
};

static BROKER: LazyLock<MessageBroker> = LazyLock::new(MessageBroker::new);

pub fn get_broker() -> &'static MessageBroker {
    &BROKER
}

/// Call this once at startup to begin dispatching messages
pub async fn start_dispatcher() {
    BROKER.start_dispatcher().await;
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

    // our use case is broadcast to all
    pub fn forward_to_room(&self, encoded_msg: Vec<u8>) -> Result<(), RoomError> {
        self.room
            .send_timeout(OneToOne::from(encoded_msg), consts::BACKBONE_DEFAULT_SEND_TIMEOUT)
    }

    async fn start_dispatcher(&self) {
        let receiver = self.room.receiver();
        let registry = self.registry;
        let shutdown_flag = Arc::clone(&self.shutdown_flag);

        let handle = tokio::spawn(async move {
            loop {
                if shutdown_flag.load(Ordering::Relaxed) {
                    info!("Dispatcher received shutdown signal");
                    break;
                }

                // Use spawn_blocking for the sync crossbeam recv
                let recv_result = recv_with_timeout(receiver).await;

                match recv_result {
                    Ok(msg) => {
                        // Now we can await the async broadcast
                        let sent = registry.broadcast(&msg, None).await.unwrap_or(0);
                        if sent > 0 {
                            info!("Dispatched message to {} users", sent);
                        }
                    }
                    Err(RecvError::Timeout) => {}
                    Err(RecvError::Disconnected) => {
                        info!("Room channel disconnected, stopping dispatcher");
                        break;
                    }
                }
            }

            info!("Dispatcher task stopped");
        });

        let mut guard = self.dispatcher_handle.lock().await;
        *guard = Some(handle);
    }

    pub async fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
        let handle = self.dispatcher_handle.lock().await.take();
        if let Some(h) = handle {
            let _ = h.await;
        }
    }
}

/// Bridge sync crossbeam recv into async context
async fn recv_with_timeout(receiver: &'static dyn MessageReceiver) -> Result<OneToMany, RecvError> {
    tokio::task::spawn_blocking(move || receiver.recv_timeout(consts::BACKBONE_DEFAULT_RECV_TIMEOUT))
        .await
        .unwrap_or(Err(RecvError::Disconnected))
}
