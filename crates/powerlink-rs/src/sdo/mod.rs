// crates/powerlink-rs/src/sdo/mod.rs
pub mod asnd;
pub mod client;
pub mod client_connection;
pub mod client_manager;
pub mod command;
pub mod embedded;
pub mod embedded_client;
pub mod embedded_server;
mod handlers;
pub mod sequence;
pub mod sequence_handler;
pub mod server;
pub mod state;
pub mod transport;
#[cfg(feature = "sdo-udp")]
pub mod udp;

pub use client::SdoClient;
pub use client_manager::SdoClientManager;
pub use command::SdoCommandHandler;
pub use embedded_client::EmbeddedSdoClient;
pub use embedded_server::EmbeddedSdoServer;
pub use server::SdoServer;
#[cfg(feature = "sdo-udp")]
pub use transport::UdpTransport;
pub use transport::{AsndTransport, SdoTransport};

// Re-exported constants for SDO configuration
const OD_IDX_SDO_TIMEOUT: u16 = 0x1300;
const OD_IDX_SDO_RETRIES: u16 = 0x1302;
