pub mod error;
pub mod port;
pub mod types;
pub mod watch_manager;

pub use error::OfficeError;
pub use types::{DocType, OfficecliStatus};
pub use watch_manager::{
    DefaultProcessSpawner, OfficecliWatchManager, ProcessHandle, ProcessSpawner,
};
