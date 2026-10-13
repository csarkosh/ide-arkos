//! Length-prefixed frames: a big-endian u32 byte length, then a postcard-encoded message.
use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Read, Write};

/// Largest frame accepted. A 16 MB replay plus headroom fits; anything larger is a bug.
pub const MAX_FRAME: u32 = 64 * 1024 * 1024;

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let body = postcard::to_allocvec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = u32::try_from(body.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame too large"))?;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

/// Reads one frame. Returns `Ok(None)` only when the stream ends exactly on a frame boundary.
pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<Option<T>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    postcard::from_bytes(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMessage, Request, Response, ServerMessage, SessionId};
    use std::io::Cursor;

    #[test]
    fn round_trips_two_frames() {
        let mut buf = Vec::new();
        let a = ClientMessage { id: 1, request: Request::Heartbeat { seq: 7 } };
        let b = ClientMessage { id: 2, request: Request::Input { session: SessionId(3), bytes: b"ls\r".to_vec() } };
        write_frame(&mut buf, &a).unwrap();
        write_frame(&mut buf, &b).unwrap();
        let mut r = Cursor::new(buf);
        let a2: ClientMessage = read_frame(&mut r).unwrap().unwrap();
        let b2: ClientMessage = read_frame(&mut r).unwrap().unwrap();
        assert_eq!(a2, a);
        assert_eq!(b2, b);
        let eof: Option<ClientMessage> = read_frame(&mut r).unwrap();
        assert!(eof.is_none());
    }

    #[test]
    fn server_message_round_trip() {
        let mut buf = Vec::new();
        let m = ServerMessage::Response { id: 9, response: Response::Attached { replay: vec![1, 2, 3], alive: true } };
        write_frame(&mut buf, &m).unwrap();
        let m2: ServerMessage = read_frame(&mut Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(m2, m);
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut buf = (MAX_FRAME + 1).to_be_bytes().to_vec();
        buf.extend_from_slice(&[0u8; 8]);
        let err = read_frame::<_, ClientMessage>(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn truncated_frame_is_an_error_not_eof() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &ClientMessage { id: 1, request: Request::ListSessions }).unwrap();
        buf.truncate(buf.len() - 1);
        let err = read_frame::<_, ClientMessage>(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }
}
