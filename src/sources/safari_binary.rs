//! Bounded caller-supplied page adapters; not fetch request-body streaming.
use super::{
    run_document_operation, validate_document_target, validate_timeout, CancellationStatus,
    DocumentTarget, EvalOptions, OperationPhase, PageOperationError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_CHUNK_BYTES: u32 = 128_000;
const MAX_TOTAL_BYTES: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryDirection {
    Download,
    Upload,
}

/// No implicit total-size default: the consumer must choose a budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryTransferOptions {
    pub allow_eval: bool,
    pub chunk_bytes: u32,
    pub total_bytes: u64,
    pub timeout: u32,
    pub idle_timeout: u32,
}

impl BinaryTransferOptions {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.allow_eval, "binary transfer requires allow_eval=true");
        anyhow::ensure!(
            (1..=MAX_CHUNK_BYTES).contains(&self.chunk_bytes),
            "binary chunk bytes must be 1-128000"
        );
        anyhow::ensure!(
            self.total_bytes <= MAX_TOTAL_BYTES,
            "binary total bytes must fit an exact JavaScript integer"
        );
        validate_timeout(self.timeout)?;
        validate_timeout(self.idle_timeout)
    }
}

/// Serializable CLI handle. Advance it only after an acknowledged chunk.
/// On any ambiguous operation failure this local handle becomes unusable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryTransfer {
    pub document: DocumentTarget,
    pub id: String,
    pub direction: BinaryDirection,
    pub options: BinaryTransferOptions,
    pub offset: u64,
    pub closed: bool,
}

impl BinaryTransfer {
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_document_target(&self.document)?;
        uuid::Uuid::parse_str(&self.id)?;
        self.options.validate()?;
        anyhow::ensure!(!self.closed, "binary transfer is closed");
        anyhow::ensure!(
            self.offset <= self.options.total_bytes,
            "invalid binary offset"
        );
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BinaryChunk {
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub done: bool,
}

fn unknown() -> anyhow::Error {
    PageOperationError {
        message: "Invalid binary acknowledgement; inspect the page before retrying".into(),
        phase: OperationPhase::Unknown,
        cancellation: CancellationStatus::Unknown,
    }
    .into()
}

/// `javascript` is a factory expression `(signal) => adapter`, possibly async.
/// Downloads implement `read(max_bytes) -> Uint8Array | null`; uploads implement
/// `write(bytes, offset)`. Both implement `close()` and best-effort `abort()`.
/// Keep credentials, URLs, and response metadata entirely inside this adapter.
pub async fn open_binary_transfer(
    javascript: &str,
    document: &DocumentTarget,
    direction: BinaryDirection,
    options: &BinaryTransferOptions,
) -> anyhow::Result<BinaryTransfer> {
    options.validate()?;
    EvalOptions {
        allow_eval: options.allow_eval,
        max_chars: 1,
        timeout: options.timeout,
    }
    .validate(javascript)?;
    validate_document_target(document)?;
    let mut transfer = BinaryTransfer {
        document: document.clone(),
        id: uuid::Uuid::new_v4().to_string(),
        direction,
        options: options.clone(),
        offset: 0,
        closed: false,
    };
    operation(&mut transfer, "open", Some(javascript), None).await?;
    Ok(transfer)
}

async fn operation(
    transfer: &mut BinaryTransfer,
    action: &str,
    javascript: Option<&str>,
    hex: Option<String>,
) -> anyhow::Result<Value> {
    transfer.validate()?;
    let config = json!({
        "id":transfer.id, "action":action, "direction":transfer.direction,
        "offset":transfer.offset, "chunk_bytes":transfer.options.chunk_bytes,
        "total_bytes":transfer.options.total_bytes, "timeout":transfer.options.timeout,
        "idle_timeout":transfer.options.idle_timeout,
        "javascript":null, "hex":hex,
        "key":format!("__cider_{}",uuid::Uuid::new_v4().simple())
    });
    let source = include_str!("safari_binary.js").replace(
        "(0, eval)(config.javascript)",
        &javascript.map_or_else(|| "null".into(), |source| format!("(\n{source}\n)")),
    );
    let output = run_document_operation(
        &transfer.document,
        &config,
        &source,
        transfer.options.timeout,
    )
    .await;
    acknowledge(transfer, action, output)
}

fn acknowledge(
    transfer: &mut BinaryTransfer,
    action: &str,
    output: anyhow::Result<String>,
) -> anyhow::Result<Value> {
    let result = output.and_then(|output| {
        if output.len() > (MAX_CHUNK_BYTES as usize * 2 + 1024) {
            return Err(unknown());
        }
        let value: Value = serde_json::from_str(&output).map_err(|_| unknown())?;
        let next = value["next_offset"].as_u64();
        if value["ok"] != true
            || value["offset"].as_u64() != Some(transfer.offset)
            || next.map_or(true, |next| {
                next < transfer.offset || next > transfer.options.total_bytes
            })
            || (matches!(action, "open" | "finish" | "abort") && next != Some(transfer.offset))
        {
            return Err(unknown());
        }
        Ok(value)
    });
    if result.is_err() {
        transfer.closed = true;
    }
    result
}

pub async fn read_binary_chunk(transfer: &mut BinaryTransfer) -> anyhow::Result<BinaryChunk> {
    transfer.validate()?;
    anyhow::ensure!(
        transfer.direction == BinaryDirection::Download,
        "not a download"
    );
    let result = operation(transfer, "read", None, None).await?;
    let parsed = decode_chunk(&result, transfer);
    match parsed {
        Ok(chunk) => {
            transfer.offset += chunk.bytes.len() as u64;
            Ok(chunk)
        }
        Err(error) => {
            transfer.closed = true;
            Err(error)
        }
    }
}

fn decode_chunk(result: &Value, transfer: &BinaryTransfer) -> anyhow::Result<BinaryChunk> {
    let hex = result["hex"].as_str().ok_or_else(unknown)?;
    let done = result["done"].as_bool().ok_or_else(unknown)?;
    if hex.len() % 2 != 0
        || hex.len() / 2 > transfer.options.chunk_bytes as usize
        || hex.len() as u64 / 2 > transfer.options.total_bytes - transfer.offset
        || (hex.is_empty() != done)
    {
        return Err(unknown());
    }
    let bytes = decode_hex(hex).ok_or_else(unknown)?;
    if result["next_offset"].as_u64() != Some(transfer.offset + bytes.len() as u64) {
        return Err(unknown());
    }
    Ok(BinaryChunk {
        offset: transfer.offset,
        bytes,
        done,
    })
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    if hex.len() % 2 != 0 {
        return None;
    }
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| Some(digit(pair[0])? * 16 + digit(pair[1])?))
        .collect()
}

pub async fn write_binary_chunk(transfer: &mut BinaryTransfer, bytes: &[u8]) -> anyhow::Result<()> {
    transfer.validate()?;
    anyhow::ensure!(
        transfer.direction == BinaryDirection::Upload,
        "not an upload"
    );
    anyhow::ensure!(
        !bytes.is_empty()
            && bytes.len() <= transfer.options.chunk_bytes as usize
            && bytes.len() as u64 <= transfer.options.total_bytes - transfer.offset,
        "binary write exceeds chunk or total bounds (or is empty)"
    );
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let result = operation(transfer, "write", None, Some(hex)).await?;
    let next = transfer.offset + bytes.len() as u64;
    if result["next_offset"].as_u64() != Some(next) {
        transfer.closed = true;
        return Err(unknown());
    }
    transfer.offset = next;
    Ok(())
}

/// Acknowledges the adapter's close, not server-side transaction rollback.
pub async fn finish_binary_transfer(transfer: &mut BinaryTransfer) -> anyhow::Result<()> {
    operation(transfer, "finish", None, None).await?;
    transfer.closed = true;
    Ok(())
}

/// Best-effort AbortSignal plus adapter abort; cannot undo dispatched requests.
pub async fn abort_binary_transfer(transfer: &mut BinaryTransfer) -> anyhow::Result<()> {
    operation(transfer, "abort", None, None).await?;
    transfer.closed = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle() -> BinaryTransfer {
        BinaryTransfer {
            document: DocumentTarget {
                window_id: 7,
                document_id: uuid::Uuid::new_v4().to_string(),
                origin: "https://example.com".into(),
            },
            id: uuid::Uuid::new_v4().to_string(),
            direction: BinaryDirection::Download,
            options: BinaryTransferOptions {
                allow_eval: true,
                chunk_bytes: 256,
                total_bytes: 512,
                timeout: 1,
                idle_timeout: 2,
            },
            offset: 0,
            closed: false,
        }
    }

    #[test]
    fn exact_binary_and_acknowledgement_bounds() {
        let transfer = handle();
        let bytes: Vec<u8> = (0..=255).collect();
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let result = json!({"hex":hex,"done":false,"next_offset":256});
        assert_eq!(decode_chunk(&result, &transfer).unwrap().bytes, bytes);
        for result in [
            json!({"hex":"0","done":false,"next_offset":1}),
            json!({"hex":"gg","done":false,"next_offset":1}),
            json!({"hex":"00","done":true,"next_offset":1}),
            json!({"hex":"","done":false,"next_offset":0}),
            json!({"hex":"00","done":false,"next_offset":2}),
            json!({"hex":"00".repeat(257),"done":false,"next_offset":257}),
        ] {
            assert!(decode_chunk(&result, &transfer).is_err());
        }
        let mut exhausted = transfer;
        exhausted.offset = exhausted.options.total_bytes;
        assert!(decode_chunk(
            &json!({"hex":"00","done":false,"next_offset":513}),
            &exhausted
        )
        .is_err());
        assert!(
            decode_chunk(&json!({"hex":"","done":true,"next_offset":512}), &exhausted)
                .unwrap()
                .done
        );
    }

    #[tokio::test]
    async fn invalid_inputs_do_not_touch_safari() {
        let mut transfer = handle();
        transfer.options.allow_eval = false;
        assert!(open_binary_transfer(
            "() => ({})",
            &transfer.document,
            transfer.direction,
            &transfer.options
        )
        .await
        .is_err());
        transfer.options.allow_eval = true;
        transfer.options.chunk_bytes = MAX_CHUNK_BYTES + 1;
        assert!(transfer.validate().is_err());
        transfer.options.chunk_bytes = 1;
        transfer.options.total_bytes = MAX_TOTAL_BYTES + 1;
        assert!(transfer.validate().is_err());
        transfer.options.total_bytes = 0;
        assert!(transfer.validate().is_ok());
        transfer.direction = BinaryDirection::Upload;
        assert!(write_binary_chunk(&mut transfer, &[0, 255]).await.is_err());
        assert!(write_binary_chunk(&mut transfer, &[]).await.is_err());
        transfer.closed = true;
        assert!(abort_binary_transfer(&mut transfer).await.is_err());
    }

    #[test]
    fn invalid_or_lost_acknowledgements_poison_handles_with_unknown_outcomes() {
        for output in [
            "not JSON".into(),
            r#"{"ok":true,"offset":1,"next_offset":1}"#.into(),
            r#"{"ok":true,"offset":0}"#.into(),
            r#"{"ok":true,"offset":0,"next_offset":513}"#.into(),
            r#"{"ok":true,"offset":0,"next_offset":1}"#.into(),
            " ".repeat(MAX_CHUNK_BYTES as usize * 2 + 1025),
        ] {
            let mut transfer = handle();
            let error = acknowledge(&mut transfer, "open", Ok(output)).unwrap_err();
            assert!(matches!(
                error.downcast_ref::<PageOperationError>().unwrap().phase,
                OperationPhase::Unknown
            ));
            assert!(transfer.closed);
            assert!(transfer.validate().is_err());
        }
        let mut transfer = handle();
        assert!(acknowledge(&mut transfer, "write", Err(unknown())).is_err());
        assert!(transfer.closed);
        let mut transfer = handle();
        assert!(acknowledge(
            &mut transfer,
            "open",
            Ok(r#"{"ok":true,"offset":0,"next_offset":0}"#.into())
        )
        .is_ok());
        assert!(!transfer.closed);
        let json = serde_json::to_string(&transfer).unwrap();
        let decoded: BinaryTransfer = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, transfer.id);
        assert_eq!(decoded.offset, 0);
        assert!(decoded.validate().is_ok());
    }
}
