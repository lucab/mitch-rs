use names;
use rand;
use std::{fmt, net};

/// Identity of a swarm member.
#[derive(Clone, Debug)]
pub struct MemberInfo {
    pub(crate) id: u32,
    pub(crate) nickname: String,
    pub(crate) min_proto: u32,
    pub(crate) max_proto: u32,
    pub(crate) target: net::SocketAddr,
    pub(crate) metadata: Vec<u8>,
}

impl MemberInfo {
    /// Maximum metadata size limit (1 MiB).
    // XXX(lucab): Arbitrary metadata limit, may be bumped if required.
    pub const MAX_METADATA: usize = 1024 * 1024;

    /// Private default constructor.
    pub(crate) fn default() -> Self {
        let nickname = names::Generator::with_naming(names::Name::Plain)
            .next()
            .unwrap_or_else(|| String::from("not-available"));
        let id = rand::random::<u32>();
        Self {
            id,
            nickname,
            min_proto: super::MIN_PROTO,
            max_proto: super::MAX_PROTO,
            target: net::SocketAddr::new(net::Ipv4Addr::LOCALHOST.into(), 0),
            metadata: vec![],
        }
    }

    /// Return metadata bytes advertised by this member.
    pub fn metadata(&self) -> &[u8] {
        &self.metadata
    }

    /// Return member ID.
    pub fn id(&self) -> &u32 {
        &self.id
    }

    /// Return member nickname.
    pub fn nickname(&self) -> &str {
        &self.nickname
    }
}

impl fmt::Display for MemberInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, r#"Swarm member "{}" (ID: {})"#, self.nickname, self.id)
    }
}
