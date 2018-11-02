use super::{errors, group};
use byteorder::{NetworkEndian, WriteBytesExt};
use futures::future;
use futures::prelude::*;
use prost::Message;
use protomitch_pb;
use std::net;

pub(crate) type FuturePaylod = Box<Future<Item = Vec<u8>, Error = errors::Error> + Send>;

pub(crate) fn ping() -> FuturePaylod {
    let ping = protomitch_pb::PingMsg {};
    let value = protomitch_pb::mitch_msg::Value::Ping(ping);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

pub(crate) fn join(info: &group::MemberInfo) -> FuturePaylod {
    // XXXi: hardcoded localhost
    let addr = protomitch_pb::join_msg::Address::Ipv4(2130706433);
    let join = protomitch_pb::JoinMsg {
        id: info.id,
        address: Some(addr),
        port: info.target.port().into(),
        nickname: info.nickname.clone(),
        min_proto: info.min_proto,
        max_proto: info.max_proto,
    };
    let value = protomitch_pb::mitch_msg::Value::Join(join);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

pub(crate) fn join_info(msg: protomitch_pb::JoinMsg) -> group::MemberInfo {
    let target = net::SocketAddr::new(
        net::IpAddr::V4(net::Ipv4Addr::new(127, 0, 0, 1)),
        msg.port as u16,
    );
    group::MemberInfo {
        id: msg.id,
        nickname: msg.nickname,
        min_proto: msg.min_proto,
        max_proto: msg.max_proto,
        target,
    }
}

pub(crate) fn sync(members: &[group::MemberInfo]) -> FuturePaylod {
    let infos = members
        .iter()
        .map(|info| {
            // XXX
            let addr = protomitch_pb::join_msg::Address::Ipv4(2130706433);
            protomitch_pb::JoinMsg {
                id: info.id,
                address: Some(addr),
                port: info.target.port().into(),
                nickname: info.nickname.clone(),
                min_proto: info.min_proto,
                max_proto: info.max_proto,
            }
        }).collect();

    let sync = protomitch_pb::SyncMsg { members: infos };
    let value = protomitch_pb::mitch_msg::Value::Sync(sync);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

pub(crate) fn pull(num_members: Option<u32>) -> FuturePaylod {
    let pull = protomitch_pb::PullMsg {
        num_members: num_members.unwrap_or(0),
    };
    let value = protomitch_pb::mitch_msg::Value::Pull(pull);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

pub(crate) fn failed(id: u32) -> FuturePaylod {
    let failed = protomitch_pb::FailedMsg { id };
    let value = protomitch_pb::mitch_msg::Value::Failed(failed);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

pub(crate) fn try_parse(payload: &[u8]) -> errors::Result<protomitch_pb::MitchMsg> {
    let msg = protomitch_pb::MitchMsg::decode(payload)?;
    Ok(msg)
}

pub(crate) fn encode(msg: &protomitch_pb::MitchMsg) -> FuturePaylod {
    let msglen = msg.encoded_len();

    let mut buf = Vec::with_capacity(msglen);
    if let Err(e) = msg.encode(&mut buf) {
        let fut_err = future::err(errors::Error::from(e));
        return Box::new(fut_err);
    };

    let mut payload = Vec::with_capacity(msglen.saturating_add(4));
    if let Err(e) = payload.write_u32::<NetworkEndian>(msglen as u32) {
        let fut_err = future::err(errors::Error::from(e));
        return Box::new(fut_err);
    };

    payload.append(&mut buf);

    let fut_payload = future::ok(payload);
    Box::new(fut_payload)
}
