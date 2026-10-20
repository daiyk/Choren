//! libqi (QiMessaging) protocol: the transport NAOqi robots and Choren's virtual robot speak.
//!
//! A Rust implementation based on libqi, Aldebaran's open-source messaging library
//! (<https://github.com/aldebaran/libqi>, BSD-3-Clause; its notice is in `LICENSE-libqi`), with
//! lugu/qiloop's protocol notes. The wire format, signatures, object ids and service-directory
//! behavior follow libqi; the code is Choren's own.
pub mod client;
pub mod discovery;
pub mod message;
pub mod meta;
pub mod node;
pub mod object;
pub mod sig;
pub mod value;

pub use client::Client;
pub use node::{Node, ServiceBuilder};
pub use object::Object;
pub use sig::Sig;
pub use value::Value;
