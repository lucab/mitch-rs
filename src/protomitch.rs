use super::{errors, MemberInfo};
use byteorder::{NetworkEndian, WriteBytesExt};
use futures::future;
use futures::prelude::*;
use prost::Message;
use protomitch_pb;
use std::net;

/// Future vector of bytes, wire format.
pub(crate) type FuturePaylod = Box<Future<Item = Vec<u8>, Error = errors::Error> + Send>;

/// Try to parse a bytes slice into a mitch protobuf message.
#[inline(always)]
pub(crate) fn try_parse_mitchmsg(payload: &[u8]) -> errors::Result<protomitch_pb::MitchMsg> {
    use errors::ResultExt;
    protomitch_pb::MitchMsg::decode(payload).chain_err(|| "failed to parse mitch protobuf message")
}

pub(crate) fn join_info(msg: protomitch_pb::JoinMsg) -> errors::Result<MemberInfo> {
    // Check metadata maximum size.
    if msg.metadata.len() >= MemberInfo::MAX_METADATA {
        bail!("overlong metadata");
    }

    // Parse peer port.
    if msg.port > u32::from(::std::u16::MAX) {
        bail!("invalid peer port ({})", msg.port);
    }
    let port = msg.port as u16;

    // Parse peer address.
    let addr = match msg.address.len() {
        4 => {
            let addr = &msg.address[..4];
            let bytes = [addr[0], addr[1], addr[2], addr[3]];
            let ipv4 = net::Ipv4Addr::from(bytes);
            net::IpAddr::V4(ipv4)
        }
        16 => {
            let addr = &msg.address[..16];
            let bytes = [
                addr[0], addr[1], addr[2], addr[3], addr[4], addr[5], addr[6], addr[7], addr[8],
                addr[9], addr[10], addr[11], addr[12], addr[12], addr[14], addr[15],
            ];
            let ipv6 = net::Ipv6Addr::from(bytes);
            net::IpAddr::V6(ipv6)
        }
        _ => bail!("unexpected address length"),
    };

    let target = net::SocketAddr::new(addr, port);
    let mi = MemberInfo {
        id: msg.id,
        nickname: msg.nickname,
        min_proto: msg.min_proto,
        max_proto: msg.max_proto,
        target,
        metadata: msg.metadata,
    };
    Ok(mi)
}

/// Encode a "ping" message.
pub(crate) fn ping() -> FuturePaylod {
    let ping = protomitch_pb::PingMsg {};
    let value = protomitch_pb::mitch_msg::Value::Ping(ping);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

/// Encode a "join" message, containing a single member.
pub(crate) fn join(info: &MemberInfo) -> FuturePaylod {
    let address = match info.target.ip() {
        net::IpAddr::V4(ip4) => ip4.octets().to_vec(),
        net::IpAddr::V6(ip6) => ip6.octets().to_vec(),
    };
    let join = protomitch_pb::JoinMsg {
        id: info.id,
        address,
        port: info.target.port().into(),
        nickname: info.nickname.clone(),
        min_proto: info.min_proto,
        max_proto: info.max_proto,
        metadata: info.metadata.clone(),
    };
    let value = protomitch_pb::mitch_msg::Value::Join(join);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

/// Encode a "sync" response, containing an array of swarm members.
pub(crate) fn sync(members: &[MemberInfo]) -> FuturePaylod {
    let infos = members
        .iter()
        .map(|info| {
            let address = match info.target.ip() {
                net::IpAddr::V4(ip4) => ip4.octets().to_vec(),
                net::IpAddr::V6(ip6) => ip6.octets().to_vec(),
            };
            protomitch_pb::JoinMsg {
                id: info.id,
                address,
                port: info.target.port().into(),
                nickname: info.nickname.clone(),
                min_proto: info.min_proto,
                max_proto: info.max_proto,
                metadata: info.metadata.clone(),
            }
        }).collect();

    let sync = protomitch_pb::SyncMsg { members: infos };
    let value = protomitch_pb::mitch_msg::Value::Sync(sync);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

/// Encode a request to pull a subset of swarm member.
///
/// An argument of `None` means "all members".
pub(crate) fn pull(num_members: Option<u32>) -> FuturePaylod {
    let pull = protomitch_pb::PullMsg {
        num_members: num_members.unwrap_or(0),
    };
    let value = protomitch_pb::mitch_msg::Value::Pull(pull);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

/// Encode a notification that a member failed and left the swarm.
pub(crate) fn failed(id: u32) -> FuturePaylod {
    let failed = protomitch_pb::FailedMsg { id };
    let value = protomitch_pb::mitch_msg::Value::Failed(failed);
    let msg = protomitch_pb::MitchMsg { value: Some(value) };
    encode(&msg)
}

/// Encode a mitch protobuf message as a future vector of bytes.
pub fn encode(msg: &protomitch_pb::MitchMsg) -> FuturePaylod {
    // Inner protobuf message length.
    let msglen = msg.encoded_len();
    if msglen >= (::std::u32::MAX as usize) {
        let fut_err = future::err("overlong protobuf message".into());
        return Box::new(fut_err);
    }
    // Overall payload length.
    let payload_len = match msglen.checked_add(4) {
        Some(val) => val,
        None => {
            let fut_err = future::err("overlong message".into());
            return Box::new(fut_err);
        }
    };
    let mut payload = Vec::with_capacity(payload_len);

    // Encode protobuf message length (NetworkEndian, 32bits, unsigned).
    if let Err(e) = payload.write_u32::<NetworkEndian>(msglen as u32) {
        let fut_err = future::err(errors::Error::from(e));
        return Box::new(fut_err);
    };

    // Encode protobuf message.
    let mut buf = Vec::with_capacity(msglen);
    if let Err(e) = msg.encode(&mut buf) {
        let fut_err = future::err(errors::Error::from(e));
        return Box::new(fut_err);
    };
    payload.append(&mut buf);

    let fut_payload = future::ok(payload);
    Box::new(fut_payload)
}
