#[derive(Clone, PartialEq, Message)]
pub struct MitchMsg {
    #[prost(oneof = "mitch_msg::Value", tags = "1, 2, 3, 4, 5")]
    pub value: ::std::option::Option<mitch_msg::Value>,
}
pub mod mitch_msg {
    #[derive(Clone, Oneof, PartialEq)]
    pub enum Value {
        #[prost(message, tag = "1")]
        Ping(super::PingMsg),
        #[prost(message, tag = "2")]
        Failed(super::FailedMsg),
        #[prost(message, tag = "3")]
        Join(super::JoinMsg),
        #[prost(message, tag = "4")]
        Pull(super::PullMsg),
        #[prost(message, tag = "5")]
        Sync(super::SyncMsg),
    }
}
#[derive(Clone, PartialEq, Message)]
pub struct PingMsg {}
#[derive(Clone, PartialEq, Message)]
pub struct FailedMsg {
    #[prost(uint32, tag = "1")]
    pub id: u32,
}
#[derive(Clone, PartialEq, Message)]
pub struct JoinMsg {
    #[prost(uint32, tag = "1")]
    pub id: u32,
    #[prost(string, tag = "2")]
    pub nickname: String,
    #[prost(uint32, tag = "5")]
    pub port: u32,
    #[prost(uint32, tag = "6")]
    pub min_proto: u32,
    #[prost(uint32, tag = "7")]
    pub max_proto: u32,
    #[prost(oneof = "join_msg::Address", tags = "3, 4")]
    pub address: ::std::option::Option<join_msg::Address>,
}
pub mod join_msg {
    #[derive(Clone, Oneof, PartialEq)]
    pub enum Address {
        #[prost(uint32, tag = "3")]
        Ipv4(u32),
        #[prost(uint64, tag = "4")]
        Ipv6(u64),
    }
}
#[derive(Clone, PartialEq, Message)]
pub struct PullMsg {
    #[prost(uint32, tag = "1")]
    pub num_members: u32,
}
#[derive(Clone, PartialEq, Message)]
pub struct SyncMsg {
    #[prost(message, repeated, tag = "1")]
    pub members: ::std::vec::Vec<JoinMsg>,
}
