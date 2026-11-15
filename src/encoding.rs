//! 文本编码探测与编解码（词库 / 台账可能是 GBK）。

use crate::Result;

/// 按 `hint`（auto/utf-8/gbk）解码字节为 UTF-8 字符串。
pub fn decode(bytes: &[u8], hint: &str) -> Result<String> {
    let encoding = match hint {
        "utf-8" => encoding_rs::UTF_8,
        "gbk" => encoding_rs::GBK,
        "auto" => detect(bytes),
        other => {
            return Err(crate::Error::Msg(format!("不支持的编码提示：{other}")));
        }
    };
    let (text, _, _) = encoding.decode(bytes);
    Ok(text.into_owned())
}

/// 用 chardetng 探测字节编码（允许 UTF-8，拒绝 ISO-2022-JP）。
pub fn detect(bytes: &[u8]) -> &'static encoding_rs::Encoding {
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    detector.guess(None, chardetng::Utf8Detection::Allow)
}

/// 确定追加写入应使用的编码：空文件按 `hint`（auto/utf-8 默认 UTF-8），
/// 否则 utf-8/gbk 按提示、auto 探测现有内容。
pub fn append_encoding(existing: &[u8], hint: &str) -> Result<&'static encoding_rs::Encoding> {
    if existing.is_empty() {
        return match hint {
            "gbk" => Ok(encoding_rs::GBK),
            "utf-8" | "auto" => Ok(encoding_rs::UTF_8),
            other => Err(crate::Error::Msg(format!("不支持的编码提示：{other}"))),
        };
    }
    match hint {
        "utf-8" => Ok(encoding_rs::UTF_8),
        "gbk" => Ok(encoding_rs::GBK),
        "auto" => Ok(detect(existing)),
        other => Err(crate::Error::Msg(format!("不支持的编码提示：{other}"))),
    }
}

/// 将 UTF-8 字符串编码为目标编码的字节。
pub fn encode(text: &str, encoding: &'static encoding_rs::Encoding) -> Vec<u8> {
    let (bytes, _, _) = encoding.encode(text);
    bytes.into_owned()
}
