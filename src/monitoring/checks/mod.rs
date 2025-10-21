pub mod tcp;
pub mod http;
pub mod database;

// Re-export all check functions
pub use tcp::check_tcp_port;
pub use http::check_http_target;
pub use database::*;