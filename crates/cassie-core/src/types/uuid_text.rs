//! Native UUID text syntax, parsed into a fixed-size buffer.
pub(crate) fn parse(input: &str) -> Option<uuid::Uuid> {
    let input = if let Some(inner) = input.strip_prefix('{') {
        inner.strip_suffix('}')?
    } else {
        input
    };
    let mut bytes = [0_u8; 16];
    let mut digits = 0;
    let mut hyphen = false;
    for byte in input.bytes() {
        if byte == b'-' {
            if digits == 0 || digits == 32 || digits % 4 != 0 || hyphen {
                return None;
            }
            hyphen = true;
            continue;
        }
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        if digits == 32 {
            return None;
        }
        bytes[digits / 2] |= if digits % 2 == 0 { digit << 4 } else { digit };
        digits += 1;
        hyphen = false;
    }
    (digits == 32 && !hyphen).then(|| uuid::Uuid::from_bytes(bytes))
}
