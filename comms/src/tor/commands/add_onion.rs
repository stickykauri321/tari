// Copyright 2020, The Tari Project
//
// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
// following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
// disclaimer.
//
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
// following disclaimer in the documentation and/or other materials provided with the distribution.
//
// 3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
// products derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
// WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use crate::tor::{
    commands::TorCommand,
    error::TorClientError,
    parsers,
    parsers::ParseError,
    response::ResponseLine,
    types::{KeyBlob, KeyType, PrivateKey},
};
use std::{borrow::Cow, marker::PhantomData, net::SocketAddr, num::NonZeroU16};

pub enum AddOnionFlag {
    /// The server should not include the newly generated private key as part of the response.
    DiscardPK,
    /// Do not associate the newly created Onion Service to the current control connection.
    Detach,
    /// Client authorization is required using the "basic" method (v2 only).
    BasicAuth,
    /// Add a non-anonymous Single Onion Service. Tor checks this flag matches its configured hidden service anonymity
    /// mode.
    NonAnonymous,
    /// Close the circuit is the maximum streams allowed is reached.
    MaxStreamsCloseCircuit,
}

impl ToString for AddOnionFlag {
    fn to_string(&self) -> String {
        use AddOnionFlag::*;
        match self {
            DiscardPK => "DiscardPK".to_string(),
            Detach => "Detach".to_string(),
            BasicAuth => "BasicAuth".to_string(),
            NonAnonymous => "NonAnonymous".to_string(),
            MaxStreamsCloseCircuit => "MaxStreamsCloseCircuit".to_string(),
        }
    }
}

/// The ADD_ONION command.
///
/// This command instructs Tor to create onion hidden services.
pub struct AddOnion<'a> {
    key_type: KeyType,
    key_blob: KeyBlob,
    flags: Vec<AddOnionFlag>,
    port: (u16, Option<SocketAddr>),
    num_streams: Option<NonZeroU16>,
    _lifetime: PhantomData<&'a ()>,
}

impl AddOnion<'_> {
    pub fn new(
        key_type: KeyType,
        key_blob: KeyBlob,
        flags: Vec<AddOnionFlag>,
        port: (u16, Option<SocketAddr>),
        num_streams: Option<NonZeroU16>,
    ) -> Self
    {
        Self {
            key_type,
            key_blob,
            flags,
            port,
            num_streams,
            _lifetime: PhantomData,
        }
    }
}

impl<'a> TorCommand for AddOnion<'a> {
    type Error = TorClientError;
    type Output = AddOnionResponse<'a>;

    fn to_command_string(&self) -> Result<String, Self::Error> {
        let mut s = String::from("ADD_ONION ");

        s.push_str(self.key_type.as_tor_repr());
        s.push(':');
        s.push_str(self.key_blob.as_tor_repr());

        if self.flags.len() > 0 {
            let flags = self.flags.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(",");
            s.push_str(&format!(" Flags={}", flags));
        }

        if let Some(num_streams) = self.num_streams {
            s.push_str(&format!(" NumStreams={}", num_streams));
        }

        s.push_str(&format!(
            " Port={}{}",
            self.port.0,
            self.port.1.map(|addr| format!(",{}", addr)).unwrap_or(String::new())
        ));

        Ok(s)
    }

    fn parse_responses(&self, mut responses: Vec<ResponseLine<'_>>) -> Result<Self::Output, Self::Error> {
        let last_response = responses.pop().ok_or(TorClientError::UnexpectedEof)?;
        if let Some(err) = last_response.err() {
            return Err(TorClientError::TorCommandFailed(err.into_owned()));
        }

        let mut service_id = None;
        let mut private_key = None;

        for response in responses {
            let (key, value) = parsers::key_value(&response.value)?;
            match &*key {
                "ServiceID" => {
                    service_id = Some(Cow::from(value.into_owned()));
                },
                "PrivateKey" => {
                    let mut split = value.split(':');
                    let key = split
                        .next()
                        .ok_or(ParseError("PrivateKey field was empty".to_string()))?;

                    let value = split
                        .next()
                        .map(|v| Cow::from(v.to_owned()))
                        .ok_or(ParseError("Failed to parse private key".to_string()))?;

                    private_key = match key {
                        "ED25519-V3" => Some(PrivateKey::Ed25519V3(value)),
                        "RSA1024" => Some(PrivateKey::Rsa1024(value)),
                        k => {
                            return Err(
                                ParseError(format!("Server returned unrecognised private key type '{}'", k)).into(),
                            )
                        },
                    };
                },
                _ => {
                    // Ignore key's we don't understand
                },
            }
        }

        let service_id = service_id.ok_or(TorClientError::AddOnionNoServiceId)?;

        Ok(AddOnionResponse {
            service_id,
            private_key,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AddOnionResponse<'a> {
    pub service_id: Cow<'a, str>,
    pub private_key: Option<PrivateKey<'a>>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn to_command_string() {
        let command = AddOnion::new(
            KeyType::New,
            KeyBlob::String("this-is-a-key".to_string()),
            vec![],
            (9090, None),
            None,
        );
        assert_eq!(
            command.to_command_string().unwrap(),
            "ADD_ONION NEW:this-is-a-key Port=9090"
        );
    }
}
