use anyhow::{Context, Result};
use rosc::{OscMessage, OscPacket, OscType};
use std::net::UdpSocket;

pub struct OscSender {
    socket: UdpSocket,
    target: String,
}

impl OscSender {
    pub fn new(port: u16) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").context("Failed to bind UDP socket")?;
        Ok(Self {
            socket,
            target: format!("127.0.0.1:{port}"),
        })
    }

    /// Send a chatbox message to VRChat via OSC.
    /// `immediate` = true bypasses VRChat's keyboard UI and shows directly.
    /// `notify` = true plays a notification sound in VRChat.
    pub fn send_chatbox(&self, message: &str, immediate: bool, notify: bool) -> Result<()> {
        let msg = OscPacket::Message(OscMessage {
            addr: "/chatbox/input".to_string(),
            args: vec![
                OscType::String(message.to_string()),
                OscType::Bool(immediate),
                OscType::Bool(notify),
            ],
        });
        let buf = rosc::encoder::encode(&msg).context("Failed to encode OSC message")?;
        self.socket
            .send_to(&buf, &self.target)
            .context("Failed to send OSC packet")?;
        Ok(())
    }
}
