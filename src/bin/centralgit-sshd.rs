extern crate thrussh;
extern crate thrussh_keys;
extern crate futures;
extern crate tokio_core;
extern crate env_logger;
extern crate ring;
use std::sync::Arc;
use thrussh::*;
use thrussh::server::{Auth, Session};
use thrussh_keys::*;
use std::net::*;

#[derive(Clone)]
struct H{}

impl server::Server for H {
    type Handler = Self;
    fn new(&self, _: SocketAddr) -> Self {
        H{}
    }
}

impl server::Handler for H {
    type Error = ();
    type FutureAuth = futures::Finished<(Self, server::Auth), Self::Error>;
    type FutureUnit = futures::Finished<(Self, server::Session), Self::Error>;
    type FutureBool = futures::Finished<(Self, server::Session, bool), Self::Error>;

    fn finished_auth(self, auth: Auth) -> Self::FutureAuth {
        futures::finished((self, auth))
    }
    fn finished_bool(self, session: Session, b: bool) -> Self::FutureBool {
        futures::finished((self, session, b))
    }
    fn finished(self, session: Session) -> Self::FutureUnit {
        futures::finished((self, session))
    }

    fn auth_password(self, user: &str, password: &str) -> Self::FutureAuth {
        /* if !(user == "me" && password == "secret") { */
            
        /*     return futures::finished((self, server::Auth::Reject)); */
        /* } */
        return futures::finished((self, server::Auth::Accept));
    }

    fn auth_publickey(self, _: &str, _: &key::PublicKey) -> Self::FutureAuth {
        println!("auth_publickey");
        futures::finished((self, server::Auth::Reject))
    }
    fn data(self, channel: ChannelId, data: &[u8], mut session: server::Session) -> Self::FutureUnit {
        println!("data on channel {:?}: {:?}", channel, std::str::from_utf8(data));
        session.data(channel, None, data);
        futures::finished((self, session))
    }

    fn exec_request(self, channel: ChannelId, data: &[u8], session: Session) -> Self::FutureUnit {
        println!("exec_request");
        futures::finished((self, session))
    }

    fn shell_request(self, channel: ChannelId, session: Session) -> Self::FutureUnit {
        println!("shell_request");
        futures::finished((self, session))
    }
}


use futures::Future;
use std::io::Read;


fn main() {
    env_logger::init();
    let rand = ring::rand::SystemRandom::new();
    let mut config = thrussh::server::Config::default();
    config.connection_timeout = Some(std::time::Duration::from_secs(600));
    config.auth_rejection_time = std::time::Duration::from_secs(3);
    /* config.methods = */ 
    config.keys.push(thrussh_keys::key::KeyPair::generate(thrussh_keys::key::ED25519).unwrap());
    let config = Arc::new(config);
    let sh = H{};
    thrussh::server::run(config, "0.0.0.0:2222", sh);
}

