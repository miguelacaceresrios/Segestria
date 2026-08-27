use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Estado de un server, para mostrar algo con vida en la ventana en vez de
/// un campo de IP mudo. Se arma con el protocolo "Server List Ping" de
/// Minecraft (el mismo que usa la lista de servers del launcher oficial),
/// implementado a mano porque es un puñado de bytes, no amerita una crate.
#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub online: bool,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub motd: Option<String>,
    /// Ícono del server como data URI (`data:image/png;base64,...`), tal cual
    /// lo manda el protocolo de status — listo para usarse directo en un `<img src>`.
    pub favicon: Option<String>,
}

impl ServerStatus {
    fn offline() -> Self {
        Self {
            online: false,
            players_online: None,
            players_max: None,
            motd: None,
            favicon: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    players: PlayersInfo,
    #[serde(default)]
    description: Option<Description>,
    #[serde(default)]
    favicon: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlayersInfo {
    max: u32,
    online: u32,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Description {
    Text(String),
    Component(DescriptionComponent),
}

#[derive(Debug, Default, Deserialize)]
struct DescriptionComponent {
    #[serde(default)]
    text: String,
    #[serde(default)]
    extra: Vec<serde_json::Value>,
}

impl Description {
    fn to_plain_text(&self) -> String {
        match self {
            Description::Text(s) => s.clone(),
            Description::Component(c) => {
                let mut out = c.text.clone();
                for part in &c.extra {
                    if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                        out.push_str(t);
                    }
                }
                out
            }
        }
    }
}

/// Nunca devuelve error: si algo falla (server apagado, IP mal escrita, sin
/// red) el resultado es simplemente "offline", que es lo que la UI necesita
/// mostrar en cualquiera de esos casos.
pub async fn ping(host: &str, port: u16) -> ServerStatus {
    match timeout(CONNECT_TIMEOUT, try_ping(host, port)).await {
        Ok(Ok(status)) => status,
        _ => ServerStatus::offline(),
    }
}

async fn try_ping(host: &str, port: u16) -> Result<ServerStatus> {
    let mut stream = TcpStream::connect((host, port)).await?;

    let mut handshake = Vec::new();
    write_varint(&mut handshake, 763); // protocolo orientativo, el server ignora el valor exacto para status
    write_string(&mut handshake, host);
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1); // next state: status

    send_packet(&mut stream, 0x00, &handshake).await?;
    send_packet(&mut stream, 0x00, &[]).await?; // status request

    let (packet_id, body) = read_packet(&mut stream).await?;
    if packet_id != 0x00 {
        return Err(Error::Other(format!("respuesta inesperada del server (packet {packet_id})")));
    }

    let mut pos = 0usize;
    let json = read_string_from_bytes(&body, &mut pos)?;
    let parsed: StatusResponse = serde_json::from_str(&json)?;

    Ok(ServerStatus {
        online: true,
        players_online: Some(parsed.players.online),
        players_max: Some(parsed.players.max),
        motd: parsed.description.map(|d| d.to_plain_text()),
        favicon: parsed.favicon,
    })
}

fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value = ((value as u32) >> 7) as i32;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    write_varint(buf, s.len() as i32);
    buf.extend_from_slice(s.as_bytes());
}

async fn send_packet(stream: &mut TcpStream, packet_id: i32, body: &[u8]) -> Result<()> {
    let mut payload = Vec::new();
    write_varint(&mut payload, packet_id);
    payload.extend_from_slice(body);

    let mut framed = Vec::new();
    write_varint(&mut framed, payload.len() as i32);
    framed.extend_from_slice(&payload);

    stream.write_all(&framed).await?;
    Ok(())
}

async fn read_varint_async(stream: &mut TcpStream) -> Result<i32> {
    let mut result = 0i32;
    let mut shift = 0u32;
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        result |= ((byte[0] & 0x7F) as i32) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(Error::Other("varint demasiado largo".to_string()));
        }
    }
    Ok(result)
}

async fn read_packet(stream: &mut TcpStream) -> Result<(i32, Vec<u8>)> {
    let length = read_varint_async(stream).await? as usize;
    let mut buf = vec![0u8; length];
    stream.read_exact(&mut buf).await?;

    let mut pos = 0usize;
    let packet_id = read_varint_from_bytes(&buf, &mut pos)?;
    Ok((packet_id, buf[pos..].to_vec()))
}

fn read_varint_from_bytes(buf: &[u8], pos: &mut usize) -> Result<i32> {
    let mut result = 0i32;
    let mut shift = 0u32;
    loop {
        let byte = *buf
            .get(*pos)
            .ok_or_else(|| Error::Other("varint truncado".to_string()))?;
        *pos += 1;
        result |= ((byte & 0x7F) as i32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(Error::Other("varint demasiado largo".to_string()));
        }
    }
    Ok(result)
}

fn read_string_from_bytes(buf: &[u8], pos: &mut usize) -> Result<String> {
    let len = read_varint_from_bytes(buf, pos)? as usize;
    let end = *pos + len;
    let bytes = buf
        .get(*pos..end)
        .ok_or_else(|| Error::Other("string truncado en la respuesta del server".to_string()))?;
    let s = std::str::from_utf8(bytes)
        .map_err(|_| Error::Other("utf8 inválido en la respuesta del server".to_string()))?
        .to_string();
    *pos = end;
    Ok(s)
}
