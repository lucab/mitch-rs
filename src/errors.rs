//! Error handling.

use futures::sync::oneshot;
use native_tls;
use prost;
use std::io;
use tokio::{executor, timer};

error_chain!{
    // doc attributes are required to workaround
    // https://github.com/rust-lang-nursery/error-chain/issues/63
    foreign_links {
        Canceled(oneshot::Canceled) #[doc = "Oneshot future canceled."];
        Io(io::Error) #[doc = "I/O error."];
        ProstDecode(prost::DecodeError) #[doc = "Protobuf decoding error."];
        ProstEncode(prost::EncodeError) #[doc = "Protobuf encoding error."];
        TokioExec(executor::SpawnError) #[doc = "Tokio executor spawning error."];
        TokioTimer(timer::Error) #[doc = "Tokio timer error."];
        TLS(native_tls::Error) #[doc = "TLS error."];
    }
}
