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

/// Reads one frame. Returns `Ok(None)` only when the stream ends before any byte of a length
/// prefix, i.e. exactly on a frame boundary. A stream that ends partway through the prefix or
/// the body is an `UnexpectedEof` error.
pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<Option<T>> {
    let mut len_buf = [0u8; 4];
    let mut filled = 0;
    while filled < len_buf.len() {
        match r.read(&mut len_buf[filled..]) {
            Ok(0) => {
                if filled == 0 {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream ended inside a frame length prefix",
                ));
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
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

    #[test]
    fn partial_length_prefix_is_an_error() {
        let err = read_frame::<_, ClientMessage>(&mut Cursor::new(vec![0u8, 0])).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn version_2_messages_round_trip_and_the_old_variants_keep_their_indices() {
        use crate::{Request, Response};
        use std::path::PathBuf;
        for req in [
            Request::Upgrade { exe: PathBuf::from("/opt/homebrew/bin/arkosd") },
            Request::RelaunchClient { exe: PathBuf::from("/opt/homebrew/bin/arkos"), cwd: PathBuf::from("/p") },
        ] {
            let bytes = postcard::to_allocvec(&req).unwrap();
            assert_eq!(postcard::from_bytes::<Request>(&bytes).unwrap(), req);
        }
        let hello = Response::Hello { daemon_version: "arkosd 0.2.0 (abc1234)".into(), protocol_min: 1, protocol_max: 2 };
        let bytes = postcard::to_allocvec(&hello).unwrap();
        assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), hello);
        // postcard writes an enum's variant index first; Ok is still variant 0 and Error still 4,
        // so a version 1 window decodes every reply it can receive from this daemon.
        assert_eq!(postcard::to_allocvec(&Response::Ok).unwrap(), vec![0]);
        assert_eq!(postcard::to_allocvec(&Response::Error(String::new())).unwrap(), vec![4, 0]);
        assert_eq!(crate::PROTOCOL_VERSION, 5);
        assert_eq!(crate::PROTOCOL_MIN, 1);
    }
}
