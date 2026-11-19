//! What a body over the spill limit owes the memory bound and the temp dir
//! (REQ-RESP-06, REQ-NFR-03): an in-memory prefix, the remainder in a temp file, and a
//! file that goes when its last owner does.

use crate::request::{RequestBody, ResolvedRequest, SendSettings};
use crate::response::ResponseBody;
use crate::transport::Transport;
use crate::{ReqwestTransport, TestServer};

use super::SpillWriter;

/// A response over the spill limit keeps its first bytes in memory and puts the rest in
/// a temp file (REQ-RESP-06): `prefix` is exactly the limit, `total_len` is the whole
/// body, and the file holds THE REMAINDER only — the prefix never reaches the disk.
#[tokio::test]
async fn response_over_limit_spills_to_temp_file() {
    let server = TestServer::start().await.expect("a loopback test server");
    let body = patterned_bytes(256);
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current()).with_spill_limit(64);
    let request = ResolvedRequest::new(
        api_model::Method::Post,
        server.url().as_str(),
        Vec::new(),
        RequestBody::Bytes(body.clone()),
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    let response = transport.send(request).await.expect("the echoed response");

    let ResponseBody::Spilled {
        prefix,
        file,
        total_len,
    } = &response.body
    else {
        panic!(
            "a body over the limit spills to a temp file, got {:?}",
            response.body
        );
    };
    assert_eq!(
        prefix.len(),
        64,
        "the in-memory prefix is exactly the limit"
    );
    assert_eq!(
        *prefix,
        body[..64],
        "the prefix holds the first bytes of the body"
    );
    assert_eq!(
        *total_len, 256,
        "the spilled body still knows its full size"
    );
    let spilled = std::fs::read(file.path()).expect("the spilled bytes");
    assert_eq!(
        spilled,
        body[64..],
        "the file holds the remainder past the limit"
    );
}

/// The temp file goes when its owner does (REQ-NFR-03): clones of a spilled body share
/// one [`crate::SpilledFile`], so the file survives until the LAST clone drops — and
/// then it is gone. That deletion is `tempfile::TempPath`'s job; nothing here removes
/// files by hand.
#[tokio::test]
async fn temp_file_removed_on_drop() {
    let mut writer = SpillWriter::new(8);
    writer
        .push(b"prefix-and-tail")
        .await
        .expect("a body in memory");
    let body = writer.into_body().await.expect("the spilled body");
    let ResponseBody::Spilled { file, .. } = &body else {
        panic!("a body over the limit spills to a temp file, got {body:?}");
    };
    let path = file.path().to_path_buf();
    assert!(
        path.exists(),
        "the spill file is there while the body lives"
    );

    let second_handle = body.clone();
    let ResponseBody::Spilled {
        file: second_file, ..
    } = &second_handle
    else {
        panic!("a clone keeps the body spilled, got {second_handle:?}");
    };
    assert_eq!(
        file, second_file,
        "clones share one file handle, so the file is deleted once"
    );

    drop(body);
    assert!(path.exists(), "one clone still owns the file");
    drop(second_handle);
    assert!(
        !path.exists(),
        "the file is removed when the last handle drops"
    );
}

/// A body exactly at the limit is the boundary of the memory bound (REQ-NFR-03): no
/// byte spills and no temp file is created, so the body stays [`ResponseBody::Bytes`].
/// Chunk boundaries do not change that.
#[tokio::test]
async fn a_body_exactly_at_the_spill_limit_stays_in_memory() {
    let mut writer = SpillWriter::new(16);
    writer
        .push(b"01234567")
        .await
        .expect("the first half of a body at the limit");
    writer
        .push(b"89abcdef")
        .await
        .expect("the second half of a body at the limit");

    let body = writer.into_body().await.expect("the body");

    assert_eq!(body, ResponseBody::Bytes(b"0123456789abcdef".to_vec()));
}

/// `len` bytes of a repeating pattern: content a file's bytes can be compared against.
fn patterned_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|index| (index % 251) as u8).collect()
}
