pub mod asnd;
pub mod client;
pub mod command;
pub mod embedded;
mod handlers;
pub mod sequence;
pub mod sequence_handler;
pub mod server;
pub mod state;
pub mod transport;
#[cfg(feature = "sdo-udp")]
pub mod udp;

pub use client::SdoClient;
pub use command::SdoCommandHandler;
pub use server::SdoServer;
#[cfg(feature = "sdo-udp")]
pub use transport::UdpTransport;
pub use transport::{AsndTransport, SdoTransport};

const OD_IDX_SDO_TIMEOUT: u16 = 0x1300;
